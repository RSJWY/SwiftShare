use anyhow::Result;
use local_ip_address::{list_afinet_netifas, local_ip};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::net::IpAddr;
use std::net::TcpStream as StdTcpStream;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const SERVICE_TYPE: &str = "_swiftshare._tcp.local.";
const DISCOVER_INTERVAL_MS: u64 = 2000;
const HEARTBEAT_INTERVAL_MS: u64 = 5000;

#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub machine_id: String,
    pub name: String,
    pub ip: String,
    pub port: u16,
}

pub struct DiscoveryHandle {
    _daemon: ServiceDaemon,
    known_devices: std::sync::Arc<std::sync::Mutex<Vec<(String, u16)>>>,
    refresh_tx: std::sync::mpsc::SyncSender<()>,
}

impl DiscoveryHandle {
    pub async fn notify_offline(&self) {
        let devices = {
            let lock = self.known_devices.lock().unwrap();
            lock.clone()
        };
        for (ip, port) in devices {
            crate::transport::send_goodbye(ip, port).await;
        }
    }

    pub fn request_refresh(&self) {
        let _ = self.refresh_tx.try_send(());
    }
}

#[derive(Debug, Clone)]
struct AnnouncedService {
    instance: String,
    host_label: String,
    ip: IpAddr,
    #[allow(dead_code)]
    machine_id: String,
}

#[derive(Debug, Clone)]
struct NetInterface {
    name: String,
    ip: IpAddr,
}

pub fn start(
    app: AppHandle,
    transport: std::sync::Arc<crate::transport::TransportHandle>,
    _settings: std::sync::Arc<crate::SettingsState>,
) -> Result<DiscoveryHandle> {
    let daemon = ServiceDaemon::new()?;
    let hostname = get_hostname();
    let host_label = ensure_local_domain(&hostname);
    let interfaces = list_interfaces().unwrap_or_default();
    let interface_ips: Vec<IpAddr> = interfaces.iter().map(|iface| iface.ip).collect();
    let machine_id = transport.machine_id.clone();

    let mut announced: Vec<AnnouncedService> = Vec::new();

    // Register our service
    // Service name format: "shortid-port[hostname]"
    // shortid = first 8 chars of UUID to stay under 63-byte DNS label limit
    // Different ports get distinct service names for multi-instance support
    let short_id = &machine_id[..machine_id.len().min(8)];
    if interfaces.is_empty() {
        let ip = local_ip()?;
        let service_name = format!("{}-{}[{}]", short_id, transport.port, hostname);
        let mut props = HashMap::new();
        props.insert("mid".to_string(), machine_id.clone());
        let service = ServiceInfo::new(
            SERVICE_TYPE,
            &service_name,
            &host_label,
            ip,
            transport.port,
            props,
        )?;
        daemon.register(service.clone())?;
        announced.push(AnnouncedService {
            instance: service_name,
            host_label: host_label.clone(),
            ip,
            machine_id: machine_id.clone(),
        });
    } else {
        for iface in &interfaces {
            if is_virtual_interface(&iface.name) {
                continue;
            }
            let service_name = format!("{}-{}[{}]", short_id, transport.port, hostname);
            let mut props = HashMap::new();
            props.insert("mid".to_string(), machine_id.clone());
            let service = ServiceInfo::new(
                SERVICE_TYPE,
                &service_name,
                &host_label,
                iface.ip,
                transport.port,
                props,
            )?;
            daemon.register(service.clone())?;
            announced.push(AnnouncedService {
                instance: service_name,
                host_label: host_label.clone(),
                ip: iface.ip,
                machine_id: machine_id.clone(),
            });
        }
    }

    let receiver = daemon.browse(SERVICE_TYPE)?;
    let local_ips = interface_ips;
    let local_port = transport.port;
    let local_machine_id = transport.machine_id.clone();
    let known_devices: std::sync::Arc<std::sync::Mutex<Vec<(String, u16)>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let known_devices_thread = known_devices.clone();
    let (refresh_tx, refresh_rx) = std::sync::mpsc::sync_channel::<()>(1);

    // Periodic refresh task - re-register services to keep them alive
    let daemon_clone = daemon.clone();
    let transport_clone = transport.clone();
    let announced_clone = announced.clone();
    let machine_id_clone = machine_id.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(DISCOVER_INTERVAL_MS));
        loop {
            interval.tick().await;
            let mut props = HashMap::new();
            props.insert("mid".to_string(), machine_id_clone.clone());
            for service in &announced_clone {
                if let Ok(updated) = ServiceInfo::new(
                    SERVICE_TYPE,
                    &service.instance,
                    &service.host_label,
                    service.ip,
                    transport_clone.port,
                    props.clone(),
                ) {
                    let _ = daemon_clone.register(updated);
                }
            }
        }
    });

    // Discovery thread - runs forever
    let daemon_for_thread = daemon.clone();
    let announced_for_thread = announced.clone();
    let machine_id_for_thread = machine_id.clone();
    thread::spawn(move || {
        let mut services: HashMap<String, (Vec<IpAddr>, String, u16, String)> = HashMap::new();
        let mut last_emit = std::time::Instant::now();
        let mut last_probe = std::time::Instant::now();
        let mut last_heartbeat = std::time::Instant::now();

        // Helper: sync known_devices from services
        let sync_known = |svcs: &HashMap<String, (Vec<IpAddr>, String, u16, String)>| {
            let mut list = Vec::new();
            for (addrs, _mid, port, _name) in svcs.values() {
                if let Some(addr) = addrs.first() {
                    list.push((addr.to_string(), *port));
                }
            }
            if let Ok(mut lock) = known_devices_thread.lock() {
                *lock = list;
            }
        };

        loop {
            match receiver.recv_timeout(Duration::from_secs(5)) {
                Ok(event) => {
                    match event {
                        ServiceEvent::ServiceResolved(info) => {
                            let fullname = info.get_fullname().to_string();
                            let port = info.get_port();
                            let addrs: Vec<IpAddr> = info.get_addresses().iter().cloned().collect();

                            // Read full machine_id from TXT property, fallback to parsing service name
                            let (parsed_mid, hostname) = extract_machine_and_hostname(&fullname);
                            let machine_id = info.get_property_val_str("mid")
                                .map(|s| s.to_string())
                                .unwrap_or(parsed_mid);

                            // Skip our own services ONLY if both machine_id AND port match
                            if machine_id == local_machine_id && port == local_port {
                                continue;
                            }

                            let key = format!("{}:{}", machine_id, port);
                            let is_new = !services.contains_key(&key);
                            services.insert(key, (addrs, machine_id, port, hostname));

                            // Re-announce ourselves so the new device can discover us quickly
                            if is_new {
                                let mut props = HashMap::new();
                                props.insert("mid".to_string(), machine_id_for_thread.clone());
                                for svc in &announced_for_thread {
                                    if let Ok(updated) = ServiceInfo::new(
                                        SERVICE_TYPE,
                                        &svc.instance,
                                        &svc.host_label,
                                        svc.ip,
                                        local_port,
                                        props.clone(),
                                    ) {
                                        let _ = daemon_for_thread.register(updated);
                                    }
                                }
                            }

                            emit_devices(&app, &services, &local_ips);
                            sync_known(&services);
                            last_emit = std::time::Instant::now();
                        }
                        ServiceEvent::ServiceRemoved(_ty, fullname) => {
                            // Service name is "shortid-port[hostname]", extract the port to find matching entries
                            let instance = fullname
                                .trim_end_matches(SERVICE_TYPE)
                                .trim_end_matches('.');
                            if let Some(bracket_pos) = instance.find('[') {
                                let prefix = &instance[..bracket_pos];
                                if let Some(dash_pos) = prefix.rfind('-') {
                                    if let Ok(port) = prefix[dash_pos + 1..].parse::<u16>() {
                                        services.retain(|_, (_, _, p, _)| *p != port);
                                    }
                                }
                            }
                            emit_devices(&app, &services, &local_ips);
                            sync_known(&services);
                            last_emit = std::time::Instant::now();
                        }
                        _ => {}
                    }
                }
                Err(_) => {
                    // Timeout — fall through to probe
                }
            }

            // TCP heartbeat every 5s: connect to each known device to punch through firewalls
            if last_heartbeat.elapsed().as_millis() >= HEARTBEAT_INTERVAL_MS as u128 {
                for (addrs, _mid, port, _name) in services.values() {
                    for addr in addrs {
                        send_heartbeat(addr.to_string(), *port);
                    }
                }
                last_heartbeat = std::time::Instant::now();
            }

            // Active probe every 5s: TCP connect to each known device, remove unreachable ones
            if last_probe.elapsed().as_secs() >= 5 {
                let mut dead_keys: Vec<String> = Vec::new();
                for (key, (addrs, _mid, port, _name)) in services.iter() {
                    let reachable = addrs.iter().any(|addr| {
                        let sa = std::net::SocketAddr::new(*addr, *port);
                        StdTcpStream::connect_timeout(&sa, Duration::from_millis(500)).is_ok()
                    });
                    if !reachable {
                        dead_keys.push(key.clone());
                    }
                }
                if !dead_keys.is_empty() {
                    for key in &dead_keys {
                        services.remove(key);
                    }
                    emit_devices(&app, &services, &local_ips);
                    sync_known(&services);
                    last_emit = std::time::Instant::now();
                }
                last_probe = std::time::Instant::now();
            }

            // Periodic re-emit
            if last_emit.elapsed().as_secs() >= 5 {
                emit_devices(&app, &services, &local_ips);
                last_emit = std::time::Instant::now();
            }

            // Manual refresh request
            if refresh_rx.try_recv().is_ok() {
                emit_devices(&app, &services, &local_ips);
                last_emit = std::time::Instant::now();
            }
        }
    });

    Ok(DiscoveryHandle { _daemon: daemon, known_devices, refresh_tx })
}

fn emit_devices(app: &AppHandle, services: &HashMap<String, (Vec<IpAddr>, String, u16, String)>, local_ips: &[IpAddr]) {
    let mut unique: HashMap<String, DeviceInfo> = HashMap::new();

    for (_key, (addrs, machine_id, port, hostname)) in services {
        let mut best_addr: Option<IpAddr> = None;

        // Prefer V4 over V6 in each pass
        let mut sorted_addrs: Vec<IpAddr> = addrs.iter().filter(|a| matches!(a, IpAddr::V4(_))).cloned().collect();
        sorted_addrs.extend(addrs.iter().filter(|a| matches!(a, IpAddr::V6(_))).cloned());

        // First prefer routable non-local V4 addresses
        for addr in &sorted_addrs {
            if !local_ips.contains(addr) && is_routable(addr) {
                best_addr = Some(*addr);
                break;
            }
        }

        // Second prefer any routable address (including local)
        if best_addr.is_none() {
            for addr in &sorted_addrs {
                if is_routable(addr) {
                    best_addr = Some(*addr);
                    break;
                }
            }
        }

        // Fallback to any address
        if best_addr.is_none() && !sorted_addrs.is_empty() {
            best_addr = sorted_addrs.first().copied();
        }

        if let Some(addr) = best_addr {
            let dedup_key = format!("{}:{}", machine_id, port);
            let device = DeviceInfo {
                machine_id: machine_id.clone(),
                name: hostname.clone(),
                ip: addr.to_string(),
                port: *port,
            };

            let should_insert = match unique.get(&dedup_key) {
                None => true,
                Some(existing) => should_replace_device(existing, &device),
            };

            if should_insert {
                unique.insert(dedup_key.clone(), device);
            }
        }
    }

    let devices: Vec<DeviceInfo> = unique.values().cloned().collect();
    let _ = app.emit("device-list-updated", devices);
}

fn list_interfaces() -> Result<Vec<NetInterface>> {
    let mut result = Vec::new();
    let interfaces = list_afinet_netifas()?;
    for (name, ip) in interfaces {
        if !matches!(ip, IpAddr::V4(_) | IpAddr::V6(_)) {
            continue;
        }
        if is_loopback(&ip) {
            continue;
        }
        if is_virtual_interface(&name) {
            continue;
        }
        result.push(NetInterface { name, ip });
    }
    Ok(result)
}

fn should_replace_device(existing: &DeviceInfo, candidate: &DeviceInfo) -> bool {
    let existing_ip: Option<IpAddr> = existing.ip.parse().ok();
    let candidate_ip: Option<IpAddr> = candidate.ip.parse().ok();
    match (existing_ip, candidate_ip) {
        (Some(existing_addr), Some(candidate_addr)) => {
            rank_addr(candidate_addr) < rank_addr(existing_addr)
        }
        (None, Some(_)) => true,
        _ => false,
    }
}

fn rank_addr(addr: IpAddr) -> u8 {
    match addr {
        IpAddr::V4(v4) => {
            if v4.is_link_local() {
                20
            } else if v4.is_private() {
                0
            } else {
                10
            }
        }
        IpAddr::V6(v6) => {
            if v6.is_unicast_link_local() {
                30
            } else if v6.is_unique_local() {
                5
            } else {
                25
            }
        }
    }
}

fn is_routable(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_link_local() && !v4.is_multicast(),
        IpAddr::V6(v6) => !v6.is_loopback() && !v6.is_unicast_link_local() && !v6.is_multicast(),
    }
}

fn is_loopback(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

fn is_virtual_interface(name: &str) -> bool {
    let lowered = name.to_lowercase();
    // 排除虚拟网卡，但保留桥接网卡（如 Hyper-V 虚拟交换机的桥接）
    // 虚拟网卡特征：veth, docker, wsl, tap, tun, hamachi, zerotier, loopback, npf
    // 保留：以太网、WiFi、Ethernet、WLAN、Intel、Realtek、Broadcom 等真实网卡
    let virtual_patterns = [
        "veth", "docker", "wsl", "tap", "tun",
        "hamachi", "zerotier", "loopback", "npf",
    ];

    // 检查是否是虚拟网卡
    if virtual_patterns.iter().any(|p| lowered.contains(p)) {
        return true;
    }

    // VMware 和 VirtualBox 的虚拟网卡通常有特定的 MAC 前缀或名称
    // 但桥接模式下会使用真实网卡，所以这里不过滤
    // 只过滤明确的虚拟网卡名称
    if lowered.contains("virtual") && !lowered.contains("ethernet") {
        return true;
    }

    false
}

/// Send a heartbeat packet to help devices behind firewalls be discovered
fn send_heartbeat(target_ip: String, target_port: u16) {
    let addr = format!("{}:{}", target_ip, target_port);
    if let Ok(socket_addr) = addr.parse::<std::net::SocketAddr>() {
        if let Ok(mut stream) = StdTcpStream::connect_timeout(&socket_addr, Duration::from_millis(200)) {
            // Send heartbeat packet: MAGIC + PacketType::Heartbeat + zeros
            let mut packet = vec![0u8; 24];
            packet[0..4].copy_from_slice(b"SWFT");
            packet[4..6].copy_from_slice(&(16u16).to_be_bytes()); // PacketType::Heartbeat = 16
            let _ = stream.write_all(&packet);
        }
    }
}

fn extract_machine_and_hostname(fullname: &str) -> (String, String) {
    let instance = fullname
        .trim_end_matches(SERVICE_TYPE)
        .trim_end_matches('.');

    // Format: "shortid-port[hostname]"
    if let Some(bracket_pos) = instance.find('[') {
        if let Some(close_bracket) = instance.find(']') {
            if bracket_pos < close_bracket {
                let prefix = &instance[..bracket_pos];
                let hostname = instance[bracket_pos + 1..close_bracket].to_string();
                // prefix is "shortid-port", extract shortid as fallback machine_id
                let machine_id = if let Some(dash_pos) = prefix.rfind('-') {
                    if prefix[dash_pos + 1..].parse::<u16>().is_ok() {
                        prefix[..dash_pos].to_string()
                    } else {
                        prefix.to_string()
                    }
                } else {
                    prefix.to_string()
                };
                return (machine_id, hostname);
            }
        }
    }

    // Fallback
    (instance.to_string(), instance.to_string())
}

fn get_hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "swiftshare".to_string())
}

fn ensure_local_domain(name: &str) -> String {
    if name.ends_with(".local.") {
        name.to_string()
    } else if name.ends_with(".local") {
        format!("{}.", name)
    } else {
        format!("{}.local.", name)
    }
}
