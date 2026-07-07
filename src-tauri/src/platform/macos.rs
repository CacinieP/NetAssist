// macOS-specific implementations

use super::{ConnectionRawInfo, NetworkInterfaceInfo};
use std::collections::HashMap;
use std::net::IpAddr;

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

/// Map a BSD interface name (e.g. `en0`) to its macOS **network service name**
/// (e.g. `Wi-Fi`). The `networksetup` tool requires the service name, not the
/// interface name — passing `en0` to `-setdnsservers`/`-setv6off` silently
/// fails with "not a recognized network service".
///
/// Parses `networksetup -listallhardwareports`:
/// ```text
/// Hardware Port: Wi-Fi
/// Device: en0
/// Ethernet Address: ...
/// ```
pub fn get_network_service_for_interface(iface: &str) -> Option<String> {
    let output = super::common::exec_command("networksetup", &["-listallhardwareports"]).ok()?;
    let mut current_service: Option<String> = None;
    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("Hardware Port:") {
            current_service = Some(name.trim().to_string());
        } else if let Some(device) = trimmed.strip_prefix("Device:") {
            if device.trim() == iface {
                return current_service.clone();
            }
        }
    }
    None
}

/// Resolve the network service name for the current default interface, with a
/// sensible fallback. Used by every `networksetup` operation.
fn default_network_service() -> String {
    let iface = get_default_interface().unwrap_or_else(|_| "en0".to_string());
    get_network_service_for_interface(&iface).unwrap_or_else(|| "Wi-Fi".to_string())
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
                    if let Ok(addr) = addr_str
                        .split('%')
                        .next()
                        .unwrap_or(addr_str)
                        .parse::<IpAddr>()
                    {
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
    let lsof_output =
        super::common::exec_command("lsof", &["-i", "-n", "-P"]).unwrap_or_else(|_| String::new());
    let mut process_map: std::collections::HashMap<String, (u32, String)> =
        std::collections::HashMap::new();

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
                let conn_key = format!(
                    "{}:{}->{}:{}",
                    local_addr, local_port, remote_addr, remote_port
                );

                let (pid, process_name) = process_map
                    .get(&conn_key)
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
                    process_name: if !process_name.is_empty() {
                        Some(process_name)
                    } else {
                        None
                    },
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
    // netstat output uses "." as separator for hex IPv4 (e.g. "C0A80101.04D2")
    // but ":" for IPv6 (e.g. "[::1]:1234" or hex "::1:04D2")
    let (ip_part, port_part) = if addr.contains('.') {
        let parts: Vec<&str> = addr.split('.').collect();
        if parts.len() < 2 {
            return None;
        }
        (parts.first()?.to_string(), parts.last()?.to_string())
    } else {
        let parts: Vec<&str> = addr.split(':').collect();
        if parts.len() < 2 {
            return None;
        }
        (parts.first()?.to_string(), parts.last()?.to_string())
    };

    // Try hex IPv4 first
    if let Ok(ip_hex) = u32::from_str_radix(&ip_part, 16) {
        let ip = IpAddr::from(std::net::Ipv4Addr::from(ip_hex));
        let port = u16::from_str_radix(&port_part, 16).ok()?;
        return Some((ip, port));
    }

    // Try standard IP parse (for IPv6 or dotted-decimal IPv4)
    if let Ok(ip) = ip_part.parse::<IpAddr>() {
        let port = port_part
            .parse::<u16>()
            .or_else(|_| u16::from_str_radix(&port_part, 16))
            .ok()?;
        return Some((ip, port));
    }

    None
}

/// Flush DNS cache on macOS
pub fn flush_dns_cache() -> anyhow::Result<()> {
    // Use: dscacheutil -flushcache
    std::process::Command::new("dscacheutil")
        .args(["-flushcache"])
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
        .args(["set", interface, "DHCP"])
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
    // networksetup -setdnsservers requires the NETWORK SERVICE name (e.g. "Wi-Fi"),
    // NOT the interface name (e.g. "en0"). Passing the interface fails silently
    // with "not a recognized network service".
    let service = default_network_service();

    let mut cmd = std::process::Command::new("networksetup");
    cmd.args(["-setdnsservers", &service, primary]);
    if let Some(secondary) = secondary {
        cmd.arg(secondary);
    }

    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "Failed to set DNS servers on {}: {}",
            service,
            stderr.trim()
        ));
    }

    Ok(())
}

/// Toggle IPv6 on the primary network service between "Automatic" and "Off".
///
/// This is a real implementation (no root required): `networksetup -getinfo`
/// reads the current state, then `-setv6off` / `-setv6automatic` flips it.
/// Returns a human-readable description of what was done.
pub fn toggle_ipv6() -> anyhow::Result<String> {
    let service = default_network_service();

    // Read current IPv6 state from `networksetup -getinfo <service>`.
    let info = super::common::exec_command("networksetup", &["-getinfo", &service])?;
    let current_off = info
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case("IPv6: Off"));

    if current_off {
        // Currently Off -> enable (Automatic)
        let output = std::process::Command::new("networksetup")
            .args(["-setv6automatic", &service])
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "Failed to enable IPv6 on {}: {}",
                service,
                stderr.trim()
            ));
        }
        Ok(format!("已启用 IPv6 (Automatic) on {}", service))
    } else {
        // Currently Automatic/On -> disable (Off)
        let output = std::process::Command::new("networksetup")
            .args(["-setv6off", &service])
            .output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "Failed to disable IPv6 on {}: {}",
                service,
                stderr.trim()
            ));
        }
        Ok(format!("已关闭 IPv6 (Off) on {}", service))
    }
}

/// Reset (disable then re-enable) the primary network adapter.
///
/// `ifconfig <iface> down/up` requires root on macOS, so this is performed via
/// `osascript` which prompts for administrator privileges (native macOS auth
/// dialog). If the user cancels, osascript exits non-zero and we surface a
/// clear error.
pub fn reset_adapter() -> anyhow::Result<()> {
    let iface = get_default_interface().unwrap_or_else(|_| "en0".to_string());

    // osascript: run a privileged shell that bounces the interface.
    // down -> wait 1s -> up. Quotes are escaped for the AppleScript string.
    let script = format!(
        "do shell script \"ifconfig {} down && sleep 1 && ifconfig {} up\" with administrator privileges",
        iface, iface
    );

    let output = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = stderr.trim();
        // osascript prints "-128 User canceled" / "not authorized" on cancel/deny.
        if msg.contains("-128") || msg.to_lowercase().contains("cancel") {
            return Err(anyhow::anyhow!(
                "用户取消了管理员授权，未重置适配器 {}",
                iface
            ));
        }
        return Err(anyhow::anyhow!(
            "重置适配器 {} 失败（需要管理员权限）: {}",
            iface,
            msg
        ));
    }

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
    match std::process::Command::new("netstat")
        .args(["-b", "-I", "en0"])
        .output()
    {
        Ok(_) => {
            status.network_monitor = true;
        }
        Err(_) => {
            status
                .warnings
                .push("无法获取网络接口统计信息。请确保应用有网络访问权限。".to_string());
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

/// Read cumulative (rx_bytes, tx_bytes) for the active interface via
/// `netstat -b -I <iface>`. Mirrors the parsing previously inlined in
/// `TrafficMonitor::get_interface_stats`.
///
/// -b reports byte counts. Output columns (1-indexed):
///   1:Name 2:Mtu 3:Network 4:Address 5:Ipkts 6:Ierrs 7:Ibytes
///   8:Opkts 9:Oerrs 10:Obytes 11:Coll
/// The same interface prints multiple rows (Link / each address) that all
/// carry the SAME cumulative byte counters; we take only the FIRST data row
/// to avoid multiplying the totals.
pub fn get_interface_total_bytes() -> (u64, u64) {
    let interface = get_default_interface().unwrap_or_else(|_| "en0".to_string());

    let output = std::process::Command::new("netstat")
        .args(["-b", "-I", &interface])
        .output();

    if let Ok(result) = output {
        let content = String::from_utf8_lossy(&result.stdout);
        // skip(1) drops the header; we then take only the first data row.
        if let Some(line) = content.lines().nth(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Need at least 10 columns to read Ibytes(7) and Obytes(10).
            if parts.len() >= 10 {
                let rx = parts[6].parse::<u64>().unwrap_or(0);
                let tx = parts[9].parse::<u64>().unwrap_or(0);
                return (rx, tx);
            }
        }
    }

    (0, 0)
}

/// Get per-process network traffic statistics on macOS.
///
/// `nettop` output is **CSV** (NOT json). With `-L 2 -s 1` it emits two
/// samples one second apart; the second sample is the **delta** over that
/// interval, so we can derive a real bytes-per-second rate by using it
/// directly. We request only the columns we need with `-j`.
///
/// Output looks like:
/// ```text
/// time,,interface,state,bytes_in,bytes_out,...
/// 13:44:25,syslogd.368,,,0,5789,...
/// ```
/// Column 0 (1-indexed) is `time`, column 2 is `name.pid`, columns 5 and 6
/// are `bytes_in` / `bytes_out` deltas.
pub fn get_process_traffic_stats(
) -> anyhow::Result<std::collections::HashMap<u32, ProcessTrafficStats>> {
    use std::collections::HashMap;
    let mut stats = HashMap::new();

    // -P   : per-process summaries only
    // -j   : include only the listed columns (lowercase! -J is invalid)
    // -x   : raw numbers (no human-readable suffixes)
    // -L 2 : emit exactly 2 samples
    // -s 1 : 1 second between samples -> sample #2 is a 1s delta
    let output = std::process::Command::new("nettop")
        .args(["-P", "-j", "bytes_in,bytes_out", "-x", "-L", "2", "-s", "1"])
        .output();

    let content = match output {
        Ok(result) => String::from_utf8_lossy(&result.stdout).into_owned(),
        Err(e) => {
            tracing::warn!("Failed to run nettop: {}", e);
            return Ok(stats);
        }
    };

    // Collect only the SECOND sample block (the 1s delta). Each sample begins
    // with a header line that starts with "time".
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in content.lines() {
        if line.starts_with("time,") {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
            current.clear();
            continue;
        }
        if !line.trim().is_empty() {
            current.push(line);
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }

    // Prefer the delta (second) block; fall back to the first block.
    let sample = blocks.last().cloned().unwrap_or_default();

    for line in sample {
        // Split by comma. Verified column layout (0-indexed) for
        // `nettop -P -j bytes_in,bytes_out -x`:
        //   [0]=time  [1]="name.pid"  [2]=interface  [3]=state
        //   [4]=bytes_in  [5]=bytes_out ...
        // (The header shows `time,,interface,...` but in -P mode the process
        //  identifier is emitted in slot [1].)
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 6 {
            continue;
        }

        let name_pid = cols[1].trim();
        // Format: "<process_name>.<pid>", e.g. "syslogd.368". The process name
        // itself may contain dots (e.g. "python3.13"), so split from the RIGHT
        // on the last '.'.
        let pid = match name_pid.rsplit_once('.') {
            Some((_, pid_str)) => pid_str.trim().parse::<u32>().unwrap_or(0),
            None => 0,
        };
        if pid == 0 {
            continue;
        }
        let name = name_pid
            .rsplit_once('.')
            .map(|(n, _)| n)
            .unwrap_or(name_pid);

        let bytes_in = cols[4].trim().parse::<u64>().unwrap_or(0);
        let bytes_out = cols[5].trim().parse::<u64>().unwrap_or(0);

        stats.insert(
            pid,
            ProcessTrafficStats {
                pid,
                name: name.to_string(),
                // When derived from the delta sample these are already a 1s
                // delta, i.e. bytes/second. We expose them as a rate.
                bytes_in,
                bytes_out,
            },
        );
    }

    Ok(stats)
}

/// Per-process traffic statistics. On macOS the `bytes_in`/`bytes_out` are a
/// 1-second delta from `nettop -L 2`, so they already represent bytes/sec.
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
        .args(["-axo", "pid,comm"])
        .output()?;

    let content = String::from_utf8_lossy(&output.stdout);

    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(pid) = parts[0].parse::<u32>() {
                let name = parts[1].to_string();
                // Get full path for more accurate name
                if let Ok(path_output) = std::process::Command::new("ps")
                    .args(["-p", parts[0], "-o", "comm="])
                    .output()
                {
                    let full_name = String::from_utf8_lossy(&path_output.stdout)
                        .trim()
                        .to_string();
                    processes.insert(
                        pid,
                        if !full_name.is_empty() {
                            full_name
                        } else {
                            name
                        },
                    );
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
    if let Ok(output) = std::process::Command::new("networksetup")
        .arg("-listallnetworkservices")
        .output()
    {
        let content = String::from_utf8_lossy(&output.stdout);
        for line in content.lines().skip(1) {
            // Skip header
            diagnostics.network_setup.push(line.trim().to_string());
        }
    }

    // 2. Check DNS configuration with scutil
    if let Ok(output) = std::process::Command::new("scutil")
        .args(["--dns"])
        .output()
    {
        let content = String::from_utf8_lossy(&output.stdout);
        for line in content.lines() {
            if line.contains("nameserver") || line.contains("domain") {
                diagnostics.dns_resolution.push(line.trim().to_string());
            }
        }
    }

    // 3. Check proxy settings
    if let Ok(output) = std::process::Command::new("scutil")
        .args(["--proxy"])
        .output()
    {
        let content = String::from_utf8_lossy(&output.stdout);
        for line in content.lines() {
            if line.contains("HTTPProxy")
                || line.contains("HTTPSProxy")
                || line.contains("SOCKSProxy")
            {
                diagnostics.proxy_config.push(line.trim().to_string());
            }
        }
    }

    // 4. Check firewall status
    if let Ok(output) =
        std::process::Command::new("/usr/libexec/ApplicationFirewall/socketfilterfw")
            .arg("--getglobalstate")
            .output()
    {
        let content = String::from_utf8_lossy(&output.stdout);
        diagnostics.firewall_status.push(content.trim().to_string());
    }

    // 5. Get WiFi info if connected to WiFi
    if let Ok(output) = std::process::Command::new(
        "/System/Library/PrivateFrameworks/Apple80211.framework/Versions/Current/Resources/airport",
    )
    .args(["-I"])
    .output()
    {
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
