// Linux-specific implementations

use super::{NetworkInterfaceInfo, ConnectionRawInfo};
use std::net::IpAddr;

/// Get default gateway on Linux
pub fn get_default_gateway() -> anyhow::Result<Option<IpAddr>> {
    // Read from /proc/net/route or use netlink
    let output = super::common::exec_command("ip", &["route", "show", "default"])?;

    // Parse output to find gateway
    for line in output.lines() {
        if line.contains("default via") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(gw_str) = parts.get(2) {
                if let Ok(gw) = gw_str.parse::<IpAddr>() {
                    return Ok(Some(gw));
                }
            }
        }
    }

    Ok(None)
}

/// Get default network interface on Linux
pub fn get_default_interface() -> anyhow::Result<String> {
    // Use: ip route show default
    let output = super::common::exec_command("ip", &["route", "show", "default"])?;

    for line in output.lines() {
        if line.contains("default via") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Format: default via X.X.X.X dev eth0 ...
            if let Some(dev_index) = parts.iter().position(|&x| x == "dev") {
                if let Some(iface) = parts.get(dev_index + 1) {
                    return Ok(iface.to_string());
                }
            }
        }
    }

    // Fallback to common interface
    Ok("eth0".to_string())
}

/// Get network interfaces on Linux
pub fn get_network_interfaces() -> anyhow::Result<Vec<NetworkInterfaceInfo>> {
    let mut interfaces = Vec::new();

    // Read from /proc/net/dev
    let output = std::fs::read_to_string("/proc/net/dev")?;

    for line in output.lines().skip(2) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if let Some(name) = parts.first() {
            let name = name.trim_end_matches(':');

            // Get addresses using ip command
            if let Ok(addr_output) = super::common::exec_command("ip", &["addr", "show", name]) {
                let mut ipv4_addrs = Vec::new();
                let mut ipv6_addrs = Vec::new();

                for line in addr_output.lines() {
                    if line.contains("inet ") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if let Some(addr_str) = parts.get(1) {
                            if let Ok(addr) = addr_str.split('/').next().unwrap_or("").parse::<IpAddr>() {
                                ipv4_addrs.push(addr);
                            }
                        }
                    } else if line.contains("inet6 ") && !line.contains("scope link") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if let Some(addr_str) = parts.get(1) {
                            if let Ok(addr) = addr_str.split('/').next().unwrap_or("").parse::<IpAddr>() {
                                ipv6_addrs.push(addr);
                            }
                        }
                    }
                }

                interfaces.push(NetworkInterfaceInfo {
                    name: name.to_string(),
                    display_name: name.to_string(),
                    ipv4_addresses: ipv4_addrs,
                    ipv6_addresses: ipv6_addrs,
                    is_up: true, // TODO: Check actual status
                    is_loopback: name == "lo",
                    gateway: None,
                });
            }
        }
    }

    Ok(interfaces)
}

/// Get active connections on Linux
pub fn get_active_connections() -> anyhow::Result<Vec<ConnectionRawInfo>> {
    // Use: ss -tunap to get connections with process info
    // Or fallback to /proc/net/tcp + lsof
    let mut connections = Vec::new();

    // First try to get process info using ss (modern Linux)
    let ss_output = super::common::exec_command("ss", &["-tunap"]).unwrap_or_default();
    let mut process_map: std::collections::HashMap<String, (u32, String)> = std::collections::HashMap::new();

    for line in ss_output.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 7 {
            continue;
        }

        // ss output format: State Recv-Q Send-Q Local Address:Port Peer Address:Port Process
        // Example: ESTAB 0 0 192.168.1.1:1234 192.168.1.2:5678 users:(("chrome",pid=1234,fd=45))
        let proc_info = parts.last().unwrap_or(&"");
        if let Some(pid_str) = proc_info.split("pid=").nth(1) {
            let pid: u32 = pid_str.split(',').next().unwrap_or("0").parse().unwrap_or(0);
            if pid > 0 {
                if let Some(name) = proc_info.split('"').nth(1) {
                    // Extract local and remote addresses
                    let local = parts.get(3).unwrap_or(&"");
                    let remote = parts.get(4).unwrap_or(&"");
                    let key = format!("{}->{}", local, remote);
                    process_map.insert(key, (pid, name.to_string()));
                }
            }
        }
    }

    // Read TCP connections from /proc/net/tcp
    if let Ok(tcp_output) = std::fs::read_to_string("/proc/net/tcp") {
        for line in tcp_output.lines().skip(1) {
            if let Some(mut conn) = parse_proc_net_line(line, "TCP") {
                // Try to find process info
                let key = format!("{}:{}->{}:{}",
                    conn.local_addr, conn.local_port, conn.remote_addr, conn.remote_port);
                if let Some((pid, name)) = process_map.get(&key) {
                    conn.pid = Some(*pid);
                    conn.process_name = Some(name.clone());
                }
                connections.push(conn);
            }
        }
    }

    // Read UDP connections from /proc/net/udp
    if let Ok(udp_output) = std::fs::read_to_string("/proc/net/udp") {
        for line in udp_output.lines().skip(1) {
            if let Some(mut conn) = parse_proc_net_line(line, "UDP") {
                // Try to find process info
                let key = format!("{}:{}->{}:{}",
                    conn.local_addr, conn.local_port, conn.remote_addr, conn.remote_port);
                if let Some((pid, name)) = process_map.get(&key) {
                    conn.pid = Some(*pid);
                    conn.process_name = Some(name.clone());
                }
                connections.push(conn);
            }
        }
    }

    Ok(connections)
}

fn parse_proc_net_line(line: &str, protocol: &str) -> Option<ConnectionRawInfo> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 10 {
        return None;
    }

    let local_addr = parse_hex_addr(parts[1])?;
    let remote_addr = parse_hex_addr(parts[2])?;

    Some(ConnectionRawInfo {
        protocol: protocol.to_string(),
        local_addr: local_addr.0,
        local_port: local_addr.1,
        remote_addr: remote_addr.0,
        remote_port: remote_addr.1,
        state: parts[3].to_string(),
        pid: None,
        process_name: None,
    })
}

fn parse_hex_addr(addr: &str) -> Option<(IpAddr, u16)> {
    let parts: Vec<&str> = addr.split(':').collect();
    if parts.len() != 2 {
        return None;
    }

    let ip_hex = u32::from_str_radix(parts[0], 16).ok()?;
    let port = u16::from_str_radix(parts[1], 16).ok()?;

    let ip = IpAddr::from(std::net::Ipv4Addr::from(ip_hex));
    Some((ip, port))
}

/// Flush DNS cache on Linux
pub fn flush_dns_cache() -> anyhow::Result<()> {
    // Try systemd-resolved first
    if let Ok(_) = std::process::Command::new("systemd-resolve")
        .args(&["--flush-caches"])
        .status()
    {
        return Ok(());
    }

    // Try dnsmasq
    if let Ok(_) = std::process::Command::new("systemctl")
        .args(&["restart", "dnsmasq"])
        .status()
    {
        return Ok(());
    }

    // No DNS cache to flush
    Ok(())
}

/// Release and renew IP on Linux
pub fn release_renew_ip() -> anyhow::Result<()> {
    // Use dhclient or dhcpcd
    if let Ok(_) = std::process::Command::new("dhclient")
        .arg("-r")
        .status()
    {
        std::process::Command::new("dhclient").status()?;
    }

    Ok(())
}

/// Reset network stack on Linux
pub fn reset_network_stack() -> anyhow::Result<()> {
    // Reload network configuration
    std::process::Command::new("systemctl")
        .args(&["restart", "NetworkManager"])
        .status()?;

    Ok(())
}

/// Get DNS servers on Linux
pub fn get_dns_servers() -> anyhow::Result<Vec<String>> {
    let mut servers = Vec::new();

    // Read from /etc/resolv.conf
    if let Ok(content) = std::fs::read_to_string("/etc/resolv.conf") {
        for line in content.lines() {
            if line.starts_with("nameserver ") {
                if let Some(addr) = line.split_whitespace().nth(1) {
                    servers.push(addr.to_string());
                }
            }
        }
    }

    Ok(servers)
}

/// Check if running with root privileges
fn check_root_privileges() -> anyhow::Result<bool> {
    // Check effective user ID (euid) - root is 0
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        // Try to check if we can read /etc/shadow (only root can)
        if let Ok(metadata) = std::fs::metadata("/etc/shadow") {
            // If we can stat it, check ownership
            return Ok(metadata.uid() == 0 && unsafe { libc::geteuid() } == 0);
        }
        // Fallback: check euid directly
        return Ok(unsafe { libc::geteuid() } == 0);
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(false)
    }
}

/// Set DNS servers on Linux with privilege check
pub fn set_dns_servers(primary: &str, secondary: Option<&str>) -> anyhow::Result<()> {
    // Check for root privileges before attempting to write
    let has_root = check_root_privileges()?;
    if !has_root {
        return Err(anyhow::anyhow!(
            "Setting DNS requires root privileges. Please run with sudo or as root."
        ));
    }

    // Validate DNS server addresses
    if primary.is_empty() {
        return Err(anyhow::anyhow!("Primary DNS server cannot be empty"));
    }
    if let Some(sec) = secondary {
        if sec.is_empty() {
            return Err(anyhow::anyhow!("Secondary DNS server cannot be empty"));
        }
    }

    // Verify /etc/resolv.conf exists and is writable
    let resolv_path = std::path::Path::new("/etc/resolv.conf");
    if !resolv_path.exists() {
        return Err(anyhow::anyhow!("/etc/resolv.conf does not exist"));
    }

    // Check if we can write to it (metadata check)
    if let Ok(metadata) = std::fs::metadata(resolv_path) {
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::PermissionsExt;
            let readonly = metadata.permissions().mode() & 0o444 == 0o444;
            if readonly {
                tracing::warn!("/etc/resolv.conf appears to be read-only");
            }
        }
    }

    // Update /etc/resolv.conf
    let mut content = String::from("# Generated by NetAssist\n");
    content.push_str(&format!("nameserver {}\n", primary));
    if let Some(secondary) = secondary {
        content.push_str(&format!("nameserver {}\n", secondary));
    }

    std::fs::write(resolv_path, content)?;
    tracing::info!("DNS servers updated successfully: primary={}, secondary={:?}", primary, secondary);
    Ok(())
}
