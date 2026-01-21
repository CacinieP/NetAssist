// macOS-specific implementations

use super::{NetworkInterfaceInfo, ConnectionRawInfo};
use std::net::IpAddr;
use std::collections::HashMap;

/// Get default gateway on macOS
pub fn get_default_gateway() -> anyhow::Result<Option<IpAddr>> {
    // Use: netstat -nr | grep default
    let output = super::common::exec_command("netstat", &["-nr"])?;

    for line in output.lines() {
        if line.contains("default") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() > 1 {
                if let Ok(gw) = parts[1].parse::<IpAddr>() {
                    return Ok(Some(gw));
                }
            }
        }
    }

    Ok(None)
}

/// Get default network interface on macOS
pub fn get_default_interface() -> anyhow::Result<String> {
    // Use: route -n get default
    let output = super::common::exec_command("route", &["-n", "get", "default"])?;

    for line in output.lines() {
        if line.contains("interface:") {
            if let Some(iface) = line.split_whitespace().nth(1) {
                return Ok(iface.to_string());
            }
        }
    }

    // Fallback to common default interface
    Ok("en0".to_string())
}

/// Get network interfaces on macOS
pub fn get_network_interfaces() -> anyhow::Result<Vec<NetworkInterfaceInfo>> {
    // Use: ifconfig
    let output = super::common::exec_command("ifconfig", &[])?;
    let mut interfaces = Vec::new();

    let mut current_interface: Option<NetworkInterfaceInfo> = None;

    for line in output.lines() {
        if line.ends_with(':') && !line.starts_with('\t') {
            if let Some(intf) = current_interface.take() {
                interfaces.push(intf);
            }

            let name = line.trim_end_matches(':').to_string();
            let is_loopback = name == "lo0";
            current_interface = Some(NetworkInterfaceInfo {
                name: name.clone(),
                display_name: name,
                ipv4_addresses: Vec::new(),
                ipv6_addresses: Vec::new(),
                is_up: false,
                is_loopback,
                gateway: None,
            });
        } else if let Some(ref mut intf) = current_interface {
            if line.contains("inet ") && !line.contains("inet6") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(addr_str) = parts.get(1) {
                    if let Ok(addr) = addr_str.parse::<IpAddr>() {
                        intf.ipv4_addresses.push(addr);
                    }
                }
            } else if line.contains("inet6 ") && !line.contains("scopeid") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(addr_str) = parts.get(1) {
                    if let Ok(addr) = addr_str.split('%').next().unwrap_or(addr_str).parse::<IpAddr>() {
                        intf.ipv6_addresses.push(addr);
                    }
                }
            } else if line.contains("status: active") {
                intf.is_up = true;
            }
        }
    }

    if let Some(intf) = current_interface {
        interfaces.push(intf);
    }

    Ok(interfaces)
}

/// Get active connections on macOS
pub fn get_active_connections() -> anyhow::Result<Vec<ConnectionRawInfo>> {
    // Use: lsof -i -n -P to get connections with process info
    let lsof_output = super::common::exec_command("lsof", &["-i", "-n", "-P"]).unwrap_or_else(|_| String::new());
    let mut process_map: std::collections::HashMap<String, (u32, String)> = std::collections::HashMap::new();

    // Parse lsof output to get process info per connection
    // Format: COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME
    for line in lsof_output.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 10 {
            continue;
        }

        let process_name = parts[0].to_string();
        let pid: u32 = parts[1].parse().unwrap_or(0);
        if pid == 0 {
            continue;
        }

        // The connection endpoint is in the last column
        if let Some(endpoint) = parts.last() {
            // Parse endpoint format: "TCP 192.168.1.1:1234->192.168.1.2:5678 (ESTABLISHED)"
            // or "UDP *:1234"
            if let Some(key) = extract_connection_key(endpoint) {
                process_map.insert(key, (pid, process_name));
            }
        }
    }

    // Use: netstat -an for basic connection info
    let output = super::common::exec_command("netstat", &["-an"])?;
    let mut connections = Vec::new();

    for line in output.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }

        let protocol = if parts[0].starts_with("tcp") {
            "TCP"
        } else if parts[0].starts_with("udp") {
            "UDP"
        } else {
            continue;
        };

        if let Some((local_addr, local_port)) = parse_sock_addr(parts[3]) {
            if let Some((remote_addr, remote_port)) = parse_sock_addr(parts[4]) {
                // Create connection key to lookup process info
                let conn_key = format!("{}:{}->{}:{}",
                    local_addr, local_port, remote_addr, remote_port);

                let (pid, process_name) = process_map.get(&conn_key)
                    .or_else(|| process_map.get(&format!("{}:{}->*", local_addr, local_port)))
                    .cloned()
                    .unwrap_or((0, String::new()));

                connections.push(ConnectionRawInfo {
                    protocol: protocol.to_string(),
                    local_addr,
                    local_port,
                    remote_addr,
                    remote_port,
                    state: parts.get(5).unwrap_or(&"").to_string(),
                    pid: if pid > 0 { Some(pid) } else { None },
                    process_name: if !process_name.is_empty() { Some(process_name) } else { None },
                });
            }
        }
    }

    Ok(connections)
}

/// Extract a connection key from lsof endpoint string
fn extract_connection_key(endpoint: &str) -> Option<String> {
    let endpoint = endpoint.trim();
    if !endpoint.starts_with("TCP") && !endpoint.starts_with("UDP") {
        return None;
    }

    // Remove protocol prefix and state suffix
    let parts: Vec<&str> = endpoint.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }

    let conn_part = parts[0]; // "192.168.1.1:1234->192.168.1.2:5678"
    Some(conn_part.to_string())
}

fn parse_sock_addr(addr: &str) -> Option<(IpAddr, u16)> {
    let parts: Vec<&str> = addr.split('.').collect();
    if parts.len() < 2 {
        return None;
    }

    let ip_str = parts.first()?;
    let port_str = parts.last()?;

    // Parse IP (hex format)
    let ip_hex = u32::from_str_radix(ip_str, 16).ok()?;
    let ip = IpAddr::from(std::net::Ipv4Addr::from(ip_hex));

    // Parse port (hex format)
    let port = u16::from_str_radix(port_str, 16).ok()?;

    Some((ip, port))
}

/// Flush DNS cache on macOS
pub fn flush_dns_cache() -> anyhow::Result<()> {
    // Use: dscacheutil -flushcache
    std::process::Command::new("dscacheutil")
        .args(&["-flushcache"])
        .status()?;

    // Also kill mDNSResponder for macOS 10.10+
    let _ = std::process::Command::new("killall")
        .arg("mDNSResponder")
        .status();

    Ok(())
}

/// Release and renew IP on macOS
pub fn release_renew_ip() -> anyhow::Result<()> {
    // Use: ipconfig set (interface) DHCP
    // Get primary interface
    let output = super::common::exec_command("route", &["-n", "get", "default"])?;
    let mut interface = "en0";

    for line in output.lines() {
        if line.contains("interface:") {
            if let Some(iface) = line.split_whitespace().nth(1) {
                interface = iface;
            }
        }
    }

    // Release
    let _ = std::process::Command::new("ipconfig")
        .args(&["set", interface, "DHCP"])
        .status();

    Ok(())
}

/// Reset network stack on macOS
pub fn reset_network_stack() -> anyhow::Result<()> {
    // Restart network service
    std::process::Command::new("killall")
        .arg("mDNSResponder")
        .status()?;

    Ok(())
}

/// Get DNS servers on macOS
pub fn get_dns_servers() -> anyhow::Result<Vec<String>> {
    // Use: scutil --dns
    let output = super::common::exec_command("scutil", &["--dns"])?;
    let mut servers = Vec::new();

    for line in output.lines() {
        if line.contains("nameserver[0]") || line.contains("nameserver[1]") {
            if let Some(addr) = line.split_whitespace().nth(1) {
                servers.push(addr.to_string());
            }
        }
    }

    if servers.is_empty() {
        // Fallback to /etc/resolv.conf
        if let Ok(content) = std::fs::read_to_string("/etc/resolv.conf") {
            for line in content.lines() {
                if line.starts_with("nameserver ") {
                    if let Some(addr) = line.split_whitespace().nth(1) {
                        servers.push(addr.to_string());
                    }
                }
            }
        }
    }

    Ok(servers)
}

/// Set DNS servers on macOS
pub fn set_dns_servers(primary: &str, secondary: Option<&str>) -> anyhow::Result<()> {
    // Use: networksetup -setdnsservers (interface) primary [secondary]
    let output = super::common::exec_command("route", &["-n", "get", "default"])?;
    let mut interface = "en0";

    for line in output.lines() {
        if line.contains("interface:") {
            if let Some(iface) = line.split_whitespace().nth(1) {
                interface = iface;
            }
        }
    }

    let mut cmd = std::process::Command::new("networksetup");
    cmd.args(&["-setdnsservers", interface, primary]);
    if let Some(secondary) = secondary {
        cmd.arg(secondary);
    }

    cmd.status()?;

    Ok(())
}

/// Check if the app has necessary permissions on macOS
pub fn check_permissions() -> anyhow::Result<PermissionStatus> {
    let mut status = PermissionStatus {
        full_disk_access: false,
        accessibility: false,
        network_monitor: false,
        warnings: Vec::new(),
    };

    // Try to run lsof to check Full Disk Access
    match std::process::Command::new("lsof").arg("-i").output() {
        Ok(_output) => {
            // If lsof runs successfully, we have Full Disk Access
            status.full_disk_access = true;
        }
        Err(_) => {
            status.warnings.push(
                "需要「完全磁盘访问权限」才能显示网络连接的进程信息。请在系统设置 > 隐私与安全性 > 完全磁盘访问权限中添加此应用。".to_string()
            );
        }
    }

    // Check if we can read network interface stats
    match std::process::Command::new("netstat").args(&["-b", "-I", "en0"]).output() {
        Ok(_) => {
            status.network_monitor = true;
        }
        Err(_) => {
            status.warnings.push(
                "无法获取网络接口统计信息。请确保应用有网络访问权限。".to_string()
            );
        }
    }

    Ok(status)
}

/// Permission status information
#[derive(Debug, Clone, serde::Serialize)]
pub struct PermissionStatus {
    pub full_disk_access: bool,
    pub accessibility: bool,
    pub network_monitor: bool,
    pub warnings: Vec<String>,
}

/// Get per-process network traffic statistics on macOS
pub fn get_process_traffic_stats() -> anyhow::Result<std::collections::HashMap<u32, ProcessTrafficStats>> {
    use std::collections::HashMap;
    let mut stats = HashMap::new();

    // Use nettop command to get real-time process traffic
    // nettop -P -J -k time,interface,protocol,bytes_in,bytes_out,process_name -L 1
    let output = std::process::Command::new("nettop")
        .args(&["-P", "-J", "-L", "1"])
        .output();

    if let Ok(result) = output {
        let content = String::from_utf8_lossy(&result.stdout);

        // Parse JSON output from nettop
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(array) = json.as_array() {
                for entry in array {
                    // Extract process info
                    let process = entry.get("process");
                    let process_name = process
                        .and_then(|p| p.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("[unknown]")
                        .to_string();

                    let pid = process
                        .and_then(|p| p.get("pid"))
                        .and_then(|p| p.as_u64())
                        .unwrap_or(0) as u32;

                    if pid == 0 {
                        continue;
                    }

                    // Extract traffic info
                    let bytes_in = entry.get("bytes_in")
                        .and_then(|b| b.as_u64())
                        .unwrap_or(0);

                    let bytes_out = entry.get("bytes_out")
                        .and_then(|b| b.as_u64())
                        .unwrap_or(0);

                    stats.insert(pid, ProcessTrafficStats {
                        pid,
                        name: process_name,
                        bytes_in,
                        bytes_out,
                    });
                }
            }
        }
    }

    Ok(stats)
}

/// Process traffic statistics
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProcessTrafficStats {
    pub pid: u32,
    pub name: String,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

/// Get all running processes on macOS
pub fn get_all_processes() -> anyhow::Result<HashMap<u32, String>> {
    let mut processes = HashMap::new();

    let output = std::process::Command::new("ps")
        .args(&["-axo", "pid,comm"])
        .output()?;

    let content = String::from_utf8_lossy(&output.stdout);

    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(pid) = parts[0].parse::<u32>() {
                let name = parts[1].to_string();
                // Get full path for more accurate name
                if let Ok(path_output) = std::process::Command::new("ps")
                    .args(&["-p", &parts[0], "-o", "comm="])
                    .output()
                {
                    let full_name = String::from_utf8_lossy(&path_output.stdout).trim().to_string();
                    processes.insert(pid, if !full_name.is_empty() { full_name } else { name });
                }
            }
        }
    }

    Ok(processes)
}

/// Monitor network interface changes
pub fn detect_interface_changes() -> anyhow::Result<InterfaceChangeEvent> {
    let current_interfaces = get_network_interfaces()?;

    // Check for new interfaces or status changes
    let mut event = InterfaceChangeEvent {
        timestamp: chrono::Utc::now().timestamp_millis(),
        changes: Vec::new(),
    };

    // Get previously known interfaces (could be stored in state)
    // For now, just check which interfaces are up
    for intf in &current_interfaces {
        if intf.is_up && !intf.is_loopback {
            event.changes.push(InterfaceChange {
                interface_name: intf.name.clone(),
                change_type: "active".to_string(),
                ipv4: intf.ipv4_addresses.first().map(|ip| ip.to_string()),
                ipv6: intf.ipv6_addresses.first().map(|ip| ip.to_string()),
            });
        }
    }

    Ok(event)
}

/// Network interface change event
#[derive(Debug, Clone, serde::Serialize)]
pub struct InterfaceChangeEvent {
    pub timestamp: i64,
    pub changes: Vec<InterfaceChange>,
}

/// Individual interface change
#[derive(Debug, Clone, serde::Serialize)]
pub struct InterfaceChange {
    pub interface_name: String,
    pub change_type: String, // "active", "inactive", "added", "removed"
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
}

/// Run macOS-specific network diagnostics
pub fn run_network_diagnostics() -> anyhow::Result<MacOSDiagnostics> {
    let mut diagnostics = MacOSDiagnostics {
        timestamp: chrono::Utc::now().timestamp_millis(),
        network_setup: Vec::new(),
        dns_resolution: Vec::new(),
        proxy_config: Vec::new(),
        firewall_status: Vec::new(),
        wifi_info: None,
    };

    // 1. Check networksetup list of services
    if let Ok(output) = std::process::Command::new("networksetup").arg("-listallnetworkservices").output() {
        let content = String::from_utf8_lossy(&output.stdout);
        for line in content.lines().skip(1) { // Skip header
            diagnostics.network_setup.push(line.trim().to_string());
        }
    }

    // 2. Check DNS configuration with scutil
    if let Ok(output) = std::process::Command::new("scutil").args(&["--dns"]).output() {
        let content = String::from_utf8_lossy(&output.stdout);
        for line in content.lines() {
            if line.contains("nameserver") || line.contains("domain") {
                diagnostics.dns_resolution.push(line.trim().to_string());
            }
        }
    }

    // 3. Check proxy settings
    if let Ok(output) = std::process::Command::new("scutil").args(&["--proxy"]).output() {
        let content = String::from_utf8_lossy(&output.stdout);
        for line in content.lines() {
            if line.contains("HTTPProxy") || line.contains("HTTPSProxy") || line.contains("SOCKSProxy") {
                diagnostics.proxy_config.push(line.trim().to_string());
            }
        }
    }

    // 4. Check firewall status
    if let Ok(output) = std::process::Command::new("/usr/libexec/ApplicationFirewall/socketfilterfw").arg("--getglobalstate").output() {
        let content = String::from_utf8_lossy(&output.stdout);
        diagnostics.firewall_status.push(content.trim().to_string());
    }

    // 5. Get WiFi info if connected to WiFi
    if let Ok(output) = std::process::Command::new("/System/Library/PrivateFrameworks/Apple80211.framework/Versions/Current/Resources/airport").args(&["-I"]).output() {
        let content = String::from_utf8_lossy(&output.stdout);
        let mut wifi_info = WiFiInfo::default();

        for line in content.lines() {
            if line.contains("agrCtrlRSSI:") {
                if let Some(value) = line.split(':').nth(1) {
                    wifi_info.rssi = value.trim().to_string();
                }
            } else if line.contains("SSID:") {
                if let Some(value) = line.split(':').nth(1) {
                    wifi_info.ssid = value.trim().to_string();
                }
            } else if line.contains("channel:") {
                if let Some(value) = line.split(':').nth(1) {
                    wifi_info.channel = value.trim().to_string();
                }
            }
        }

        if !wifi_info.ssid.is_empty() {
            diagnostics.wifi_info = Some(wifi_info);
        }
    }

    Ok(diagnostics)
}

/// macOS-specific diagnostics
#[derive(Debug, Clone, serde::Serialize)]
pub struct MacOSDiagnostics {
    pub timestamp: i64,
    pub network_setup: Vec<String>,
    pub dns_resolution: Vec<String>,
    pub proxy_config: Vec<String>,
    pub firewall_status: Vec<String>,
    pub wifi_info: Option<WiFiInfo>,
}

/// WiFi information
#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct WiFiInfo {
    pub ssid: String,
    pub rssi: String,    // Signal strength
    pub channel: String, // WiFi channel
}
