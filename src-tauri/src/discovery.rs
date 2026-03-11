use anyhow::Result;
use local_ip_address::{list_afinet_netifas, local_ip};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;
use std::collections::HashMap;
use std::net::IpAddr;
use std::thread;
use tauri::{AppHandle, Emitter};

const SERVICE_TYPE: &str = "_swiftshare._tcp.local.";

#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub name: String,
    pub ip: String,
    pub port: u16,
}

pub struct DiscoveryHandle {
    _daemon: ServiceDaemon,
}

#[derive(Debug, Clone)]
struct NetInterface {
    name: String,
    ip: IpAddr,
}

pub fn start(app: AppHandle, port: u16) -> Result<DiscoveryHandle> {
    let daemon = ServiceDaemon::new()?;
    let hostname = get_hostname();
    let host_label = ensure_local_domain(&hostname);
    let interfaces = list_interfaces().unwrap_or_default();
    let interface_ips = interfaces.iter().map(|iface| iface.ip).collect::<Vec<_>>();

    if interfaces.is_empty() {
        let ip = local_ip()?;
        let service = ServiceInfo::new(
            SERVICE_TYPE,
            &hostname,
            &host_label,
            ip,
            port,
            HashMap::new(),
        )?;
        daemon.register(service)?;
    } else {
        for iface in &interfaces {
            let service_name = format!("{}::{}", hostname, iface.name);
            let service = ServiceInfo::new(
                SERVICE_TYPE,
                &service_name,
                &host_label,
                iface.ip,
                port,
                HashMap::new(),
            )?;
            daemon.register(service)?;
        }
    }

    let receiver = daemon.browse(SERVICE_TYPE)?;
    let local_ips = interface_ips;

    thread::spawn(move || {
        let mut services: HashMap<String, DeviceInfo> = HashMap::new();

        while let Ok(event) = receiver.recv() {
            match event {
                ServiceEvent::ServiceResolved(info) => {
                    if let Some((addr, port)) = pick_addr_and_port(&info, &local_ips) {
                        if local_ips.contains(&addr) {
                            continue;
                        }
                        let base_name = base_service_name(&info);
                        let fullname = info.get_fullname().to_string();
                        let device = DeviceInfo {
                            name: format!("{}{}", base_name, SERVICE_TYPE),
                            ip: addr.to_string(),
                            port,
                        };
                        services.insert(fullname, device);
                        emit_devices(&app, &services);
                    }
                }
                ServiceEvent::ServiceRemoved(_ty, fullname) => {
                    services.remove(&fullname);
                    emit_devices(&app, &services);
                }
                _ => {}
            }
        }
    });

    Ok(DiscoveryHandle { _daemon: daemon })
}

fn pick_addr_and_port(info: &ServiceInfo, local_ips: &[IpAddr]) -> Option<(IpAddr, u16)> {
    let port = info.get_port();
    let mut addrs = info.get_addresses().iter().cloned().collect::<Vec<_>>();
    addrs.sort_by_key(|addr| match addr {
        IpAddr::V4(_) => 0,
        IpAddr::V6(_) => 1,
    });
    if let Some(addr) = addrs
        .iter()
        .find(|addr| !local_ips.contains(addr) && is_routable(*addr))
    {
        return Some((*addr, port));
    }
    addrs.into_iter().next().map(|addr| (addr, port))
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

fn emit_devices(app: &AppHandle, services: &HashMap<String, DeviceInfo>) {
    let mut aggregated: HashMap<String, DeviceInfo> = HashMap::new();
    for device in services.values() {
        let key = base_name_from_fullname(&device.name);
        if let Some(existing) = aggregated.get_mut(&key) {
            if should_replace_device(existing, device) {
                *existing = device.clone();
            }
        } else {
            aggregated.insert(key, device.clone());
        }
    }
    let _ = app.emit(
        "device-list-updated",
        aggregated.values().cloned().collect::<Vec<_>>(),
    );
}

fn base_service_name(info: &ServiceInfo) -> String {
    let fullname = info.get_fullname().to_string();
    base_name_from_fullname(&fullname)
}

fn base_name_from_fullname(fullname: &str) -> String {
    let instance = fullname
        .trim_end_matches(SERVICE_TYPE)
        .trim_end_matches('.');
    instance.split("::").next().unwrap_or(instance).to_string()
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
        "virtual",
        "vmware",
        "hyper-v",
        "vbox",
        "virtualbox",
        "veth",
        "docker",
        "wsl",
        "bridge",
        "tap",
        "tun",
        "hamachi",
        "zerotier",
        "loopback",
        "npf",
    ];
    patterns
        .iter()
        .any(|pattern| lowered.contains(&pattern.to_lowercase()))
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
