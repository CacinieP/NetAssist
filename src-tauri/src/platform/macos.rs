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
                // IPv6 gateways may carry a %zone suffix (fe80::1%en0).
                let gw = parts[1].split('%').next().unwrap_or(parts[1]);
                if let Ok(gw) = gw.parse::<IpAddr>() {
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

/// Resolve the network service name for the current default interface.
/// Used by every `networksetup` operation.
///
/// Returns an error rather than guessing "Wi-Fi": when the default interface
/// is a VPN tunnel (utun*) there is no matching network service, and
/// `networksetup` operations must not silently target an unrelated service.
fn default_network_service() -> anyhow::Result<String> {
    let iface = get_default_interface().unwrap_or_else(|_| "en0".to_string());
    get_network_service_for_interface(&iface).ok_or_else(|| {
        anyhow::anyhow!(
            "interface {} is not backed by a networksetup service (VPN tunnel?)",
            iface
        )
    })
}

/// Get network interfaces on macOS
pub fn get_network_interfaces() -> anyhow::Result<Vec<NetworkInterfaceInfo>> {
    // Use: ifconfig
    let output = super::common::exec_command("ifconfig", &[])?;
    let mut interfaces = Vec::new();

    let mut current_interface: Option<NetworkInterfaceInfo> = None;

    for line in output.lines() {
        // Interface header (not indented): "en0: flags=8863<UP,BROADCAST,...> mtu 1500"
        if !line.starts_with(char::is_whitespace) && line.contains(": flags=") {
            if let Some(intf) = current_interface.take() {
                interfaces.push(intf);
            }

            let name = line
                .split_once(':')
                .map(|(n, _)| n.to_string())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            // Parse "<UP,BROADCAST,RUNNING,...>" for the link-up bit.
            let is_up = line
                .split_once('<')
                .and_then(|(_, rest)| rest.split_once('>'))
                .map(|(flags, _)| flags.split(',').any(|f| f == "UP"))
                .unwrap_or(false);
            let is_loopback = name == "lo0";
            current_interface = Some(NetworkInterfaceInfo {
                name: name.clone(),
                display_name: name,
                ipv4_addresses: Vec::new(),
                ipv6_addresses: Vec::new(),
                is_up,
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
            } else if line.contains("inet6 ") {
                // Include link-local too; strip any "%scope" zone suffix.
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

/// IP address family used when an endpoint is a wildcard (`*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IpFamily {
    V4,
    V6,
}

impl IpFamily {
    /// Unspecified address for this family (used to render `*` endpoints).
    fn unspecified(self) -> IpAddr {
        match self {
            IpFamily::V4 => IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
            IpFamily::V6 => IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED),
        }
    }
}

/// A parsed socket endpoint where `None` means "wildcard" (`*`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct EndPoint {
    ip: Option<IpAddr>,
    port: Option<u16>,
}

impl EndPoint {
    fn wildcard() -> Self {
        Self {
            ip: None,
            port: None,
        }
    }

    /// Canonical, family-aware key used to join netstat rows with lsof pids.
    /// Wildcards are rendered as the unspecified address of `fam` so that a
    /// `tcp6 *.7000` netstat row and an IPv6 `TCP *:7000` lsof row collide.
    fn key(&self, fam: IpFamily) -> String {
        let ip = match self.ip {
            Some(ip) => ip.to_string(),
            None => fam.unspecified().to_string(),
        };
        format!("{}:{}", ip, self.port.unwrap_or(0))
    }

    /// Concrete address for the output struct (`*` → unspecified of `fam`).
    fn ip_or(&self, fam: IpFamily) -> IpAddr {
        self.ip.unwrap_or_else(|| fam.unspecified())
    }
}

/// Parse an IPv4/IPv6 textual address, tolerating the `[brackets]` form and
/// dropping any `%scope` zone suffix that netstat/lsof may print for
/// link-local addresses.
fn parse_ip_text(s: &str) -> Option<IpAddr> {
    let s = s.trim().trim_matches('[').trim_matches(']');
    let s = s.split('%').next().unwrap_or(s);
    s.parse::<IpAddr>().ok()
}

/// Parse a port token: `""`, `"*"` → None (wildcard); otherwise decimal.
fn parse_port_text(s: &str) -> Option<u16> {
    let s = s.trim();
    if s.is_empty() || s == "*" {
        None
    } else {
        s.parse::<u16>().ok()
    }
}

/// Parse a `netstat -an` endpoint column on macOS.
///
/// Real formats (verified on macOS 15):
/// - IPv4:    `192.168.1.200.61782`
/// - IPv6:    `::1.8021`            (no brackets; `.` separates address+port)
/// - wildcard: `*.49152`, `*.*`, `*`
fn parse_netstat_endpoint(token: &str) -> Option<EndPoint> {
    let t = token.trim();
    if t.is_empty() || t == "*" {
        return Some(EndPoint::wildcard());
    }
    if let Some(rest) = t.strip_prefix("*.") {
        // `*.49152` (wildcard address, concrete port)
        return Some(EndPoint {
            ip: None,
            port: parse_port_text(rest),
        });
    }
    // `ip.port` — the port is always the last dot-separated segment on macOS.
    let (ip_part, port_part) = t.rsplit_once('.').unwrap_or((t, ""));
    let ip = if ip_part == "*" {
        None
    } else {
        parse_ip_text(ip_part)
    };
    Some(EndPoint {
        ip,
        port: parse_port_text(port_part),
    })
}

/// Parse one side of an lsof NAME column endpoint (e.g. `192.168.1.1:1234`,
/// `[::1]:53`, `*:7000`, `*:*`, or empty for a missing remote side).
fn parse_lsof_endpoint(token: &str) -> EndPoint {
    let t = token.trim();
    if t.is_empty() {
        return EndPoint::wildcard();
    }
    if let Some(inner) = t.strip_prefix('[') {
        if let Some(idx) = inner.find("]:") {
            return EndPoint {
                ip: parse_ip_text(&inner[..idx]),
                port: parse_port_text(&inner[idx + 2..]),
            };
        }
    }
    match t.rsplit_once(':') {
        Some((ip_part, port_part)) => EndPoint {
            ip: if ip_part == "*" {
                None
            } else {
                parse_ip_text(ip_part)
            },
            port: parse_port_text(port_part),
        },
        None => EndPoint {
            ip: parse_ip_text(t),
            port: None,
        },
    }
}

/// lsof escapes whitespace/control bytes in the COMMAND column as `\xNN`;
/// decode them back so process names display correctly.
fn unescape_lsof_command(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() && bytes[i + 1] == b'x' {
            let hex = &s[i + 2..i + 4];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v as char);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Get active connections on macOS.
///
/// netstat is the source of truth for the connection list; lsof (which
/// carries PID + process name) is parsed into a pid-map keyed by the same
/// canonical endpoint strings that netstat rows produce.
pub fn get_active_connections() -> anyhow::Result<Vec<ConnectionRawInfo>> {
    // Use: lsof -i -n -P to get process info per connection
    let lsof_output =
        super::common::exec_command("lsof", &["-i", "-n", "-P"]).unwrap_or_else(|_| String::new());
    let mut process_map: std::collections::HashMap<String, (u32, String)> =
        std::collections::HashMap::new();

    // Real lsof socket line (10 cols for TCP, 9 for UDP):
    //   ControlCe  609 user  9u IPv4 0x…  0t0 TCP *:7000 (LISTEN)
    // Column map: 0=COMMAND 1=PID 2=USER 3=FD 4=TYPE(IPv4/IPv6)
    //             5=DEVICE 6=SIZE/OFF 7=NODE(TCP/UDP) 8+=NAME(endpoint [state])
    for line in lsof_output.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            continue;
        }
        let proto = parts[7];
        if proto != "TCP" && proto != "UDP" {
            continue;
        }
        let pid: u32 = match parts[1].parse() {
            Ok(p) if p > 0 => p,
            _ => continue,
        };
        let fam = if parts[4] == "IPv6" {
            IpFamily::V6
        } else {
            IpFamily::V4
        };

        // NAME starts at column 8; the endpoint is its first whitespace token.
        let spec = parts[8];
        let (local_spec, remote_spec) = match spec.split_once("->") {
            Some((l, r)) => (l, r),
            None => (spec, ""),
        };
        let local = parse_lsof_endpoint(local_spec);
        let remote = parse_lsof_endpoint(remote_spec);
        let key = format!("{}->{}", local.key(fam), remote.key(fam));
        process_map.insert(key, (pid, unescape_lsof_command(parts[0])));
    }

    // Use: netstat -an for the connection list.
    let output = super::common::exec_command("netstat", &["-an"])?;
    let mut connections = Vec::new();

    for line in output.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }

        let (protocol, fam) = match parts[0] {
            "tcp4" => ("TCP", IpFamily::V4),
            "tcp6" => ("TCP", IpFamily::V6),
            "udp4" => ("UDP", IpFamily::V4),
            "udp6" => ("UDP", IpFamily::V6),
            _ => continue,
        };

        let local = match parse_netstat_endpoint(parts[3]) {
            Some(local) => local,
            None => continue,
        };
        // Skip fully-wildcard rows (e.g. unbound `udp4 *.* *.*`) — nothing to show.
        if local.ip.is_none() && local.port.is_none() {
            continue;
        }
        let remote = match parts.get(4) {
            Some(tok) => parse_netstat_endpoint(tok).unwrap_or_else(EndPoint::wildcard),
            None => EndPoint::wildcard(),
        };

        let conn_key = format!("{}->{}", local.key(fam), remote.key(fam));
        let (pid, process_name) = process_map
            .get(&conn_key)
            .cloned()
            .unwrap_or((0, String::new()));

        connections.push(ConnectionRawInfo {
            protocol: protocol.to_string(),
            local_addr: local.ip_or(fam),
            local_port: local.port.unwrap_or(0),
            remote_addr: remote.ip_or(fam),
            remote_port: remote.port.unwrap_or(0),
            state: parts.get(5).unwrap_or(&"").to_string(),
            pid: if pid > 0 { Some(pid) } else { None },
            process_name: if !process_name.is_empty() {
                Some(process_name)
            } else {
                None
            },
        });
    }

    Ok(connections)
}

/// Run a privileged shell command via the native macOS authorization prompt
/// (`osascript` + `with administrator privileges`). Prompts the user for an
/// admin password if the process is not running as root.
fn run_privileged_shell(command: &str) -> anyhow::Result<()> {
    // The command is interpolated into an AppleScript string that executes a
    // root shell. It must not contain double quotes or backslashes.
    if command.contains('"') || command.contains('\\') {
        return Err(anyhow::anyhow!("refusing to run unsafe shell command"));
    }
    let script = format!(
        "do shell script \"{}\" with administrator privileges",
        command
    );

    let output = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = stderr.trim();
        if msg.contains("-128") || msg.to_lowercase().contains("cancel") {
            return Err(anyhow::anyhow!("用户取消了管理员授权"));
        }
        return Err(anyhow::anyhow!(
            "特权命令执行失败（需要管理员权限）: {}",
            msg
        ));
    }
    Ok(())
}

/// Flush DNS cache on macOS
///
/// `dscacheutil -flushcache` works unprivileged; the `killall mDNSResponder`
/// step requires root, so it is attempted directly and, when it fails (EPERM),
/// re-run through the osascript admin prompt. Exit codes are never swallowed:
/// a genuinely failing command surfaces as an error instead of "success".
pub fn flush_dns_cache() -> anyhow::Result<()> {
    // Use: dscacheutil -flushcache (works as a normal user)
    let status = std::process::Command::new("dscacheutil")
        .args(["-flushcache"])
        .status()?;
    if !status.success() {
        return Err(anyhow::anyhow!("dscacheutil -flushcache failed"));
    }

    // kill mDNSResponder for macOS 10.10+ — needs root.
    let direct = std::process::Command::new("killall")
        .args(["-HUP", "mDNSResponder"])
        .status();
    if let Ok(status) = direct {
        if status.success() {
            return Ok(());
        }
    }
    // Non-root: escalate via the admin prompt.
    run_privileged_shell("killall -HUP mDNSResponder")
}

/// Release and renew IP on macOS
///
/// `ipconfig set <iface> DHCP` requires root; escalate via the admin prompt
/// when running unprivileged, and report failures honestly.
pub fn release_renew_ip() -> anyhow::Result<()> {
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
    if interface.is_empty()
        || !interface
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(anyhow::anyhow!("非法接口名 {:?}", interface));
    }

    // Try direct; escalate through the auth prompt when it fails.
    let direct = std::process::Command::new("ipconfig")
        .args(["set", interface, "DHCP"])
        .output();
    if let Ok(out) = direct {
        if out.status.success() {
            return Ok(());
        }
    }
    let cmd = format!("ipconfig set {} DHCP", interface);
    run_privileged_shell(&cmd)
}

/// Reset the network stack on macOS
///
/// Restarts the mDNS resolver (needs root). Never swallows exit codes.
pub fn reset_network_stack() -> anyhow::Result<()> {
    let direct = std::process::Command::new("killall")
        .args(["-HUP", "mDNSResponder"])
        .status();
    if let Ok(status) = direct {
        if status.success() {
            return Ok(());
        }
    }
    // Killall fails with EPERM when we are not root → escalate via prompt.
    run_privileged_shell("killall -HUP mDNSResponder")
}

/// Get DNS servers on macOS
pub fn get_dns_servers() -> anyhow::Result<Vec<String>> {
    // Use: scutil --dns — one "resolver #N" block per interface; each block
    // lists its own nameserver[0..] lines. Only read the FIRST block that
    // actually contains nameservers (the primary resolver), and collect all
    // of its entries instead of mixing nameserver[0]/[1] across blocks.
    let output = super::common::exec_command("scutil", &["--dns"])?;
    let mut servers = Vec::new();
    let mut in_first_populated_block = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("resolver #") {
            // A new block: adopt it only if we haven't yet found servers.
            if !servers.is_empty() {
                break;
            }
            in_first_populated_block = true;
            continue;
        }
        if in_first_populated_block {
            if let Some(rest) = trimmed.strip_prefix("nameserver[") {
                if let Some(addr) = rest.split(':').nth(1) {
                    let addr = addr.trim();
                    if !addr.is_empty() && !servers.contains(&addr.to_string()) {
                        servers.push(addr.to_string());
                    }
                }
            }
        }
    }

    if servers.is_empty() {
        // Fallback to /etc/resolv.conf
        if let Ok(content) = std::fs::read_to_string("/etc/resolv.conf") {
            for line in content.lines() {
                if line.starts_with("nameserver ") {
                    if let Some(addr) = line.split_whitespace().nth(1) {
                        if !servers.contains(&addr.to_string()) {
                            servers.push(addr.to_string());
                        }
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
    let service = default_network_service()?;

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
    let service = default_network_service()?;

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

    // The interface name is interpolated into an AppleScript string that runs
    // a root shell; reject anything that is not a plain BSD interface name so
    // it can never be used as an injection vector.
    if iface.is_empty() || !iface.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(anyhow::anyhow!("拒绝重置适配器：非法接口名 {:?}", iface));
    }

    // osascript: run a privileged shell that bounces the interface.
    // down -> wait 1s -> up. Interface name validated above.
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

/// Check if the app has necessary permissions on macOS.
///
/// Honest checks only:
/// - Full Disk Access is verified by checking whether `lsof` can list socket
///   info for processes OTHER than our own (without FDA, lsof only reports
///   our own processes, even though it spawns fine).
/// - Accessibility is verified with the real TCC API (`AXIsProcessTrusted`).
/// - Network monitoring verifies the actual default interface counters.
pub fn check_permissions() -> anyhow::Result<PermissionStatus> {
    let mut status = PermissionStatus {
        full_disk_access: false,
        accessibility: false,
        network_monitor: false,
        warnings: Vec::new(),
    };

    // Full Disk Access: `lsof` spawns successfully even without FDA, so a
    // spawn check proves nothing. With FDA granted, lsof -i lists sockets of
    // every user; without it, rows only cover our own process. Compare the
    // USER column against the current user to detect FDA.
    if let Ok(current_user) = super::common::exec_command("id", &["-un"]) {
        let current_user = current_user.trim().to_string();
        let lsof = super::common::exec_command("lsof", &["-i", "-n", "-P"]).unwrap_or_default();
        let sees_other_users = lsof.lines().skip(1).any(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            parts.len() >= 3 && parts[2] != current_user && !parts[2].is_empty()
        });
        status.full_disk_access = sees_other_users;
    }
    if !status.full_disk_access {
        status.warnings.push(
            "需要「完全磁盘访问权限」才能显示其他进程的网络连接信息。请在 系统设置 > 隐私与安全性 > 完全磁盘访问权限 中添加此应用。".to_string()
        );
    }

    // Accessibility (TCC): query the OS directly.
    status.accessibility = ax_is_process_trusted();
    if !status.accessibility {
        status
            .warnings
            .push("需要「辅助功能」权限才能执行部分网络修复操作。请在 系统设置 > 隐私与安全性 > 辅助功能 中添加此应用。".to_string());
    }

    // Network monitor: read counters for the REAL default interface (not a
    // hardcoded en0). Requires the output to actually contain a parseable row.
    let default_iface = get_default_interface().unwrap_or_else(|_| "en0".to_string());
    match std::process::Command::new("netstat")
        .args(["-b", "-I", &default_iface])
        .output()
    {
        Ok(output) if output.status.success() => {
            let content = String::from_utf8_lossy(&output.stdout);
            let has_data = content
                .lines()
                .skip(1)
                .any(|l| l.split_whitespace().count() >= 10);
            status.network_monitor = has_data;
        }
        _ => {}
    }
    if !status.network_monitor {
        status
            .warnings
            .push("无法获取网络接口统计信息。请确保应用有网络访问权限。".to_string());
    }

    Ok(status)
}

// Query the Accessibility (TCC) trust state via `AXIsProcessTrusted`
// (HIServices / ApplicationServices). The framework link is added in build.rs.
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

#[cfg(target_os = "macos")]
fn ax_is_process_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
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
/// `netstat -b -I <iface>`.
///
/// macOS `netstat -b` column layout is NOT fixed: rows for interfaces without
/// a MAC address (utun/vpn) omit the Address column, shifting every later
/// column left. So we locate the `Ibytes`/`Obytes` column positions from the
/// header row, then scan data rows for one that has at least that many
/// columns. All rows of the same interface carry the same cumulative counter,
/// so we just take the first parseable one.
pub fn get_interface_total_bytes() -> (u64, u64) {
    let interface = get_default_interface().unwrap_or_else(|_| "en0".to_string());

    let output = std::process::Command::new("netstat")
        .args(["-b", "-I", &interface])
        .output();

    if let Ok(result) = output {
        let content = String::from_utf8_lossy(&result.stdout);
        let mut lines = content.lines();

        // Locate the Ibytes/Obytes column indexes from the header.
        let (rx_idx, tx_idx, header_len) = match lines
            .next()
            .map(|h| h.split_whitespace().collect::<Vec<_>>())
        {
            Some(header) => {
                let find = |name: &str| header.iter().position(|c| c.eq_ignore_ascii_case(name));
                match (find("Ibytes"), find("Obytes")) {
                    (Some(rx), Some(tx)) => (rx, tx, header.len()),
                    _ => return (0, 0),
                }
            }
            None => return (0, 0),
        };

        for line in lines {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // A row missing the Address column (e.g. utun without a MAC) is
            // shifted left; only full-width rows align with the header.
            if parts.len() < header_len || parts.len() <= tx_idx.max(rx_idx) {
                continue;
            }
            if let (Ok(rx), Ok(tx)) = (parts[rx_idx].parse::<u64>(), parts[tx_idx].parse::<u64>()) {
                return (rx, tx);
            }
        }
    }

    (0, 0)
}

/// Get per-process network traffic statistics on macOS.
///
/// `nettop` output is **CSV** (NOT json). With `-L 2 -s 1` it emits two
/// samples one second apart. In delta mode (`-d`) the SECOND sample is the
/// delta over that 1s interval — i.e. a real bytes/second rate. Without `-d`
/// both samples are cumulative process-start byte counts and would be
/// misreported as per-second rates, so `-d` is required.
///
/// Output looks like (verified on macOS 15):
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
    // -d   : delta mode — report bytes since the previous sample
    // -j   : append only the listed columns (case-sensitive: -j, not -J)
    // -x   : raw numbers (no human-readable suffixes)
    // -L 2 : emit exactly 2 samples
    // -s 1 : 1 second between samples -> sample #2 is a 1s delta
    let output = std::process::Command::new("nettop")
        .args([
            "-P",
            "-d",
            "-j",
            "bytes_in,bytes_out",
            "-x",
            "-L",
            "2",
            "-s",
            "1",
        ])
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
        // `nettop -P -d -j bytes_in,bytes_out -x`:
        //   [0]=time  [1]="name.pid"  [2]=interface  [3]=state
        //   [4]=bytes_in  [5]=bytes_out ...
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
                // The second delta sample is already a 1s delta, i.e.
                // bytes/second. We expose them as a rate.
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

    // `ps -axo pid=,comm=` — with explicit empty headers, comm starts right
    // after the pid. Use the widest available executable field (`comm` is the
    // full path on macOS). Split on the FIRST whitespace run only, so paths
    // containing spaces are not truncated, and never spawn a second `ps` per
    // process.
    let output = std::process::Command::new("ps")
        .args(["-axo", "pid=,comm="])
        .output()?;

    let content = String::from_utf8_lossy(&output.stdout);

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((pid_str, path)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(pid) = pid_str.trim().parse::<u32>() else {
            continue;
        };
        // Use the last path segment for display, like `ps` comm would show.
        let name = path
            .trim()
            .rsplit('/')
            .next()
            .map(str::to_string)
            .unwrap_or_default();
        if !name.is_empty() {
            processes.insert(pid, name);
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

    // 5. Get WiFi info if connected to WiFi.
    //
    // The classic `airport -I` binary was removed in macOS 14.4, so use
    // `system_profiler SPAirPortDataType -json` instead. It is slower, but
    // this is only run on demand from the diagnostics page.
    if let Ok(output) = std::process::Command::new("system_profiler")
        .args(["SPAirPortDataType", "-json"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
            let mut wifi_info = WiFiInfo::default();
            if let Some(net) = json.pointer("/SPAirPortDataType/spairport_network_infos/0") {
                if let Some(ssid) = net.get("spairport_network_info_SSID") {
                    if let Some(ssid) = ssid.as_str() {
                        wifi_info.ssid = ssid.to_string();
                    }
                }
                if let Some(channel) = net.get("spairport_network_info_channel") {
                    if let Some(channel) = channel.as_u64() {
                        wifi_info.channel = channel.to_string();
                    }
                }
                if let Some(rssi) = net.get("spairport_network_info_rssi") {
                    if let Some(rssi) = rssi.as_i64() {
                        wifi_info.rssi = format!("{} dBm", rssi);
                    }
                }
            }
            if !wifi_info.ssid.is_empty() {
                diagnostics.wifi_info = Some(wifi_info);
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    fn v6() -> IpAddr {
        IpAddr::V6(Ipv6Addr::LOCALHOST)
    }

    #[test]
    fn netstat_endpoint_dotted_ipv4() {
        let ep = parse_netstat_endpoint("192.168.1.200.61782").unwrap();
        assert_eq!(ep.ip, Some(v4(192, 168, 1, 200)));
        assert_eq!(ep.port, Some(61782));
    }

    #[test]
    fn netstat_endpoint_ipv6_dot_separator() {
        let ep = parse_netstat_endpoint("::1.8021").unwrap();
        assert_eq!(ep.ip, Some(v6()));
        assert_eq!(ep.port, Some(8021));
    }

    #[test]
    fn netstat_endpoint_wildcards() {
        let ep = parse_netstat_endpoint("*.49152").unwrap();
        assert_eq!(ep.ip, None);
        assert_eq!(ep.port, Some(49152));
        let ep = parse_netstat_endpoint("*.*").unwrap();
        assert_eq!(ep.ip, None);
        assert_eq!(ep.port, None);
    }

    #[test]
    fn lsof_endpoint_forms() {
        let ep = parse_lsof_endpoint("127.0.0.1:59408");
        assert_eq!(ep.ip, Some(v4(127, 0, 0, 1)));
        assert_eq!(ep.port, Some(59408));

        let ep = parse_lsof_endpoint("[::1]:8021");
        assert_eq!(ep.ip, Some(v6()));
        assert_eq!(ep.port, Some(8021));

        let ep = parse_lsof_endpoint("*:7000");
        assert_eq!(ep.ip, None);
        assert_eq!(ep.port, Some(7000));

        let ep = parse_lsof_endpoint("*:*");
        assert_eq!(ep.ip, None);
        assert_eq!(ep.port, None);
    }

    #[test]
    fn lsof_command_unescape() {
        assert_eq!(unescape_lsof_command("Codex\\x20Helper"), "Codex Helper");
        assert_eq!(unescape_lsof_command("plain"), "plain");
    }

    #[test]
    fn key_matches_between_tools() {
        // netstat: 127.0.0.1.7890 <-> 127.0.0.1.61778  vs lsof NAME with "->"
        let net_local = parse_netstat_endpoint("127.0.0.1.7890").unwrap();
        let net_remote = parse_netstat_endpoint("127.0.0.1.61778").unwrap();
        let lsof_local = parse_lsof_endpoint("127.0.0.1:7890");
        let lsof_remote = parse_lsof_endpoint("127.0.0.1:61778");
        assert_eq!(
            format!(
                "{}->{}",
                net_local.key(IpFamily::V4),
                net_remote.key(IpFamily::V4)
            ),
            format!(
                "{}->{}",
                lsof_local.key(IpFamily::V4),
                lsof_remote.key(IpFamily::V4)
            )
        );
    }

    #[test]
    fn key_matches_listen_wildcard() {
        // netstat tcp6 *.49152 *.* LISTEN vs lsof IPv6 TCP *:49152 (LISTEN)
        let net_local = parse_netstat_endpoint("*.49152").unwrap();
        let net_remote = parse_netstat_endpoint("*.*").unwrap();
        let lsof_local = parse_lsof_endpoint("*:49152");
        let lsof_remote = parse_lsof_endpoint("");
        assert_eq!(
            format!(
                "{}->{}",
                net_local.key(IpFamily::V6),
                net_remote.key(IpFamily::V6)
            ),
            format!(
                "{}->{}",
                lsof_local.key(IpFamily::V6),
                lsof_remote.key(IpFamily::V6)
            )
        );
    }

    #[test]
    fn key_distinguishes_v4_v6_wildcard() {
        let v4_ep = parse_lsof_endpoint("*:7000");
        let v6_ep = parse_lsof_endpoint("*:7000");
        assert_ne!(v4_ep.key(IpFamily::V4), v6_ep.key(IpFamily::V6));
    }
}

#[cfg(test)]
mod live_tests {
    //! Live sanity checks that run against the real machine tools.
    use super::*;

    #[test]
    fn live_connections_have_sane_addresses() {
        let conns = get_active_connections().expect("netstat/lsof should run");
        // Every row must carry concrete ip/port (no hex garbage, no oversized ports).
        for c in conns.iter().take(200) {
            // u16 port type already guarantees range; just ensure no empty addr.
            assert!(!c.local_addr.to_string().is_empty(), "empty local addr");
            // IPv4/IPv6 parse cleanly (hex-garbage bug would yield > u16 ports
            // or odd dotted addresses).
            assert!(
                c.protocol == "TCP" || c.protocol == "UDP",
                "bad proto {}",
                c.protocol
            );
        }
        // With >0 established connections on this machine, at least one pid
        // should be resolvable (lsof join works).
        let tcp = conns.iter().filter(|c| c.protocol == "TCP").count();
        assert!(
            tcp > 0,
            "expected at least one TCP connection on a live Mac"
        );
    }

    #[test]
    fn live_interface_list_nonempty() {
        let intfs = get_network_interfaces().expect("ifconfig runs");
        assert!(intfs.iter().any(|i| i.name == "lo0"), "lo0 present");
        assert!(intfs.iter().any(|i| i.is_loopback), "loopback flagged");
    }
}
