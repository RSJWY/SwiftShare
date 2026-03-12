use anyhow::Result;
use local_ip_address::{list_afinet_netifas, local_ip};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;
use serde_json;
use std::collections::HashMap;
use std::net::IpAddr;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const SERVICE_TYPE: &str = "_swiftshare._tcp.local.";
const DISCOVER_INTERVAL_MS: u64 = 2000;

#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub machine_id: String,
    pub name: String,
    pub ip: String,
    pub port: u16,
}

pub struct DiscoveryHandle {
    _daemon: ServiceDaemon,
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

    eprintln!("[Discovery] Starting with hostname: {}, machine_id: {}, port: {}", hostname, machine_id, transport.port);
    eprintln!("[Discovery] Local IPs: {:?}", interface_ips);

    let mut announced: Vec<AnnouncedService> = Vec::new();

    // Register our service
    if interfaces.is_empty() {
        let ip = local_ip()?;
        let service_name = format!("{}[{}]", machine_id, hostname);
        eprintln!("[Discovery] Registering service: {} on ip {}:{}", service_name, ip, transport.port);
        let service = ServiceInfo::new(
            SERVICE_TYPE,
            &service_name,
            &host_label,
            ip,
            transport.port,
            HashMap::new(),
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
                eprintln!("[Discovery] Skipping virtual interface: {}", iface.name);
                continue;
            }
            let service_name = format!("{}[{}]", machine_id, hostname);
            eprintln!("[Discovery] Registering service: {} on {}:{}", service_name, iface.ip, transport.port);
            let service = ServiceInfo::new(
                SERVICE_TYPE,
                &service_name,
                &host_label,
                iface.ip,
                transport.port,
                HashMap::new(),
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

    eprintln!("[Discovery] Announced {} services", announced.len());

    let receiver = daemon.browse(SERVICE_TYPE)?;
    let local_ips = interface_ips;
    let local_port = transport.port;
    let local_machine_id = transport.machine_id.clone();

    // Periodic refresh task
    let daemon_clone = daemon.clone();
    let app_clone = app.clone();
    let transport_clone = transport.clone();
    let announced_clone = announced.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(DISCOVER_INTERVAL_MS));
        loop {
            interval.tick().await;
            let list = transport_clone.shared_list().await;
            let props = build_manifest_properties(&list);
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
            let _ = app_clone.emit("shared-list-updated", list);
        }
    });

    // Discovery thread - runs forever
    thread::spawn(move || {
        eprintln!("[Discovery] Browser thread started");
        let mut services: HashMap<String, (Vec<IpAddr>, String, u16, String)> = HashMap::new();
        let mut last_emit = std::time::Instant::now();

        loop {
            match receiver.recv_timeout(Duration::from_secs(30)) {
                Ok(event) => {
                    match event {
                        ServiceEvent::ServiceResolved(info) => {
                            let fullname = info.get_fullname().to_string();
                            let port = info.get_port();
                            let addrs: Vec<IpAddr> = info.get_addresses().iter().cloned().collect();

                            eprintln!("[Discovery] Resolved: {} addrs={:?} port={}", fullname, addrs, port);

                            // Extract machine_id and hostname from service name
                            let (machine_id, hostname) = extract_machine_and_hostname(&fullname);

                            // Skip our own services
                            if machine_id == local_machine_id && port == local_port {
                                eprintln!("[Discovery]   -> SKIPPED: own service");
                                continue;
                            }

                            let key = format!("{}:{}", machine_id, port);
                            services.insert(key.clone(), (addrs, machine_id, port, hostname));
                            eprintln!("[Discovery]   -> Added service: {} (total unique: {})", key, services.len());

                            if let Some(manifest) = parse_manifest_properties(&info) {
                                let _ = app.emit("remote-manifest-updated", manifest);
                            }

                            emit_devices(&app, &services, &local_ips);
                            last_emit = std::time::Instant::now();
                        }
                        ServiceEvent::ServiceRemoved(_ty, fullname) => {
                            eprintln!("[Discovery] Removed: {}", fullname);
                            let (machine_id, _) = extract_machine_and_hostname(&fullname);
                            services.retain(|k, _| !k.starts_with(&machine_id));
                            emit_devices(&app, &services, &local_ips);
                            last_emit = std::time::Instant::now();
                        }
                        ServiceEvent::SearchStarted(_) => {
                            eprintln!("[Discovery] Search started");
                        }
                        ServiceEvent::SearchStopped(_) => {
                            eprintln!("[Discovery] Search stopped");
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    eprintln!("[Discovery] recv timeout/error: {:?}", e);
                }
            }

            // Periodic re-emit
            if last_emit.elapsed().as_secs() >= 5 {
                eprintln!("[Discovery] Periodic re-emit ({} services)", services.len());
                emit_devices(&app, &services, &local_ips);
                last_emit = std::time::Instant::now();
            }
        }
    });

    Ok(DiscoveryHandle { _daemon: daemon })
}

fn emit_devices(app: &AppHandle, services: &HashMap<String, (Vec<IpAddr>, String, u16, String)>, local_ips: &[IpAddr]) {
    let mut unique: HashMap<String, DeviceInfo> = HashMap::new();

    for (_key, (addrs, machine_id, port, hostname)) in services {
        let mut best_addr: Option<IpAddr> = None;

        // First prefer routable non-local addresses
        for addr in addrs {
            if !local_ips.contains(addr) && is_routable(addr) {
                best_addr = Some(*addr);
                break;
            }
        }

        // Second prefer local addresses
        if best_addr.is_none() {
            for addr in addrs {
                if is_routable(addr) {
                    best_addr = Some(*addr);
                    break;
                }
            }
        }

        // Fallback to any address
        if best_addr.is_none() && !addrs.is_empty() {
            best_addr = addrs.first().copied();
        }

        if let Some(addr) = best_addr {
            let dedup_key = machine_id.clone();
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
    eprintln!("[Discovery] Emitting {} devices to UI", devices.len());
    for d in &devices {
        eprintln!("[Discovery]   -> {} at {}:{}", d.name, d.ip, d.port);
    }

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
    let patterns = [
        "virtual", "vmware", "hyper-v", "vbox", "virtualbox",
        "veth", "docker", "wsl", "bridge", "tap", "tun",
        "hamachi", "zerotier", "loopback", "npf",
    ];
    patterns.iter().any(|p| lowered.contains(p))
}

#[allow(dead_code)]
fn base_service_name(info: &ServiceInfo) -> String {
    let fullname = info.get_fullname().to_string();
    base_name_from_fullname(&fullname)
}

#[allow(dead_code)]
fn base_name_from_fullname(fullname: &str) -> String {
    let instance = fullname
        .trim_end_matches(SERVICE_TYPE)
        .trim_end_matches('.');

    // Extract base name before ::
    let base = instance.split("::").next().unwrap_or(instance);

    // Remove port suffix if present (e.g., "hostname:1234" -> "hostname")
    if let Some((name, port)) = base.rsplit_once(':') {
        if port.parse::<u16>().is_ok() {
            return name.to_string();
        }
    }

    base.to_string()
}

fn extract_machine_and_hostname(fullname: &str) -> (String, String) {
    let instance = fullname
        .trim_end_matches(SERVICE_TYPE)
        .trim_end_matches('.');

    // Format is "uuid[hostname]"
    if let Some(bracket_pos) = instance.find('[') {
        if let Some(close_bracket) = instance.find(']') {
            if bracket_pos < close_bracket {
                let machine_id = instance[..bracket_pos].to_string();
                let hostname = instance[bracket_pos + 1..close_bracket].to_string();
                return (machine_id, hostname);
            }
        }
    }

    // Fallback to old format for backward compatibility
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

#[derive(Debug, Clone, Serialize)]
pub struct FileManifest {
    pub ip: String,
    pub port: u16,
    pub files: Vec<crate::transport::SharedEntry>,
}

fn build_manifest_properties(list: &[crate::transport::SharedEntry]) -> HashMap<String, String> {
    let payload = serde_json::to_string(list).unwrap_or_else(|_| "[]".to_string());
    let mut props = HashMap::new();
    props.insert("manifest".to_string(), payload);
    props
}

fn parse_manifest_properties(info: &ServiceInfo) -> Option<crate::discovery::FileManifest> {
    let ip = info.get_addresses().iter().next().copied()?;
    let port = info.get_port();
    let value = info.get_property_val_str("manifest")?;
    let files: Vec<crate::transport::SharedEntry> = serde_json::from_str(value).ok()?;
    Some(FileManifest {
        ip: ip.to_string(),
        port,
        files,
    })
}
