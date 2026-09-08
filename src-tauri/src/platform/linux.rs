// Linux-specific implementations
//
// The pure parsers live in `platform::common` so they are compiled and
// unit-tested on every host; this file only performs the file/process I/O
// around them.

use super::{ConnectionRawInfo, NetworkInterfaceInfo};
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
                let mut is_up = false;

                for line in addr_output.lines() {
                    // Interface header: "2: eth0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 ..."
                    if line.contains("mtu") && line.contains(':') {
                        if let Some(flags) = line
                            .split_once('<')
                            .and_then(|(_, r)| r.split_once('>'))
                        {
                            is_up = flags.0.split(',').any(|f| f == "UP");
                        }
                    } else if line.contains("inet ") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if let Some(addr_str) = parts.get(1) {
                            if let Ok(addr) =
                                addr_str.split('/').next().unwrap_or("").parse::<IpAddr>()
                            {
                                ipv4_addrs.push(addr);
                            }
                        }
                    } else if line.contains("inet6 ") && !line.contains("scope link") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if let Some(addr_str) = parts.get(1) {
                            if let Ok(addr) = addr_str
                                .split('/')
                                .next()
                                .unwrap_or("")
                                .split('%')
                                .next()
                                .unwrap_or("")
                                .parse::<IpAddr>()
                            {
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
                    is_up,
                    is_loopback: name == "lo",
                    gateway: None,
                });
            }
        }
    }

    Ok(interfaces)
}

/// Get active connections on Linux.
///
/// `ss -H -tuanp` is the single source of truth: with both `-t` and `-u` the
/// output carries a leading `Netid` column, human-readable state names and,
/// where permitted, the owning `users:(("name",pid=N,...))` suffix. Falling
/// back to `/proc/net/{tcp,tcp6,udp,udp6}` (byte-order corrected, all four
/// files read) only when `ss` is unavailable.
pub fn get_active_connections() -> anyhow::Result<Vec<ConnectionRawInfo>> {
    let ss = super::common::exec_command("ss", &["-H", "-t", "-u", "-a", "-n", "-p"]);
    if let Ok(ss_output) = ss {
        if !ss_output.trim().is_empty() {
            return Ok(super::common::parse_ss_connections(&ss_output));
        }
    }
    Ok(parse_proc_net_connections())
}

/// Fallback connection listing from `/proc/net/{tcp,tcp6,udp,udp6}` with
/// correct byte order and state-code mapping (pid join impossible without ss).
fn parse_proc_net_connections() -> Vec<ConnectionRawInfo> {
    let mut connections = Vec::new();
    for (file, proto, v6) in [
        ("/proc/net/tcp", "TCP", false),
        ("/proc/net/tcp6", "TCP", true),
        ("/proc/net/udp", "UDP", false),
        ("/proc/net/udp6", "UDP", true),
    ] {
        if let Ok(content) = std::fs::read_to_string(file) {
            for line in content.lines().skip(1) {
                if let Some(conn) = super::common::parse_proc_net_line(line, proto, v6) {
                    connections.push(conn);
                }
            }
        }
    }
    connections
}

/// Flush DNS cache on Linux
pub fn flush_dns_cache() -> anyhow::Result<()> {
    // resolvectl (modern) or systemd-resolve (legacy)
    for cmd in ["resolvectl", "systemd-resolve"] {
        let status = std::process::Command::new(cmd)
            .arg("--flush-caches")
            .status();
        if let Ok(status) = status {
            if status.success() {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Release and renew IP on Linux
pub fn release_renew_ip() -> anyhow::Result<()> {
    // Use dhclient or dhcpcd
    if let Ok(status) = std::process::Command::new("dhclient").arg("-r").status() {
        if status.success() {
            std::process::Command::new("dhclient")
                .arg("-1")
                .status()?;
        }
    }

    Ok(())
}

/// Reset network stack on Linux
pub fn reset_network_stack() -> anyhow::Result<()> {
    // Reload network configuration
    std::process::Command::new("systemctl")
        .args(["restart", "NetworkManager"])
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

/// Check if running with root privileges (euid == 0), read from
/// /proc/self/status so no libc dependency is required.
fn check_root_privileges() -> anyhow::Result<bool> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            // "Uid:	0	0	0	0" — first value is the real uid; the second
            // is euid. Privilege checks should use the effective uid.
            let fields: Vec<&str> = rest.split_whitespace().collect();
            return Ok(fields.get(1).copied().unwrap_or("") == "0");
        }
    }
    Ok(false)
}

/// Validate that a string is a parseable IP address (IPv4 or IPv6).
fn validate_ip(addr: &str) -> Result<(), anyhow::Error> {
    addr.parse::<IpAddr>()
        .map(|_| ())
        .map_err(|_| anyhow::anyhow!("invalid IP address: {}", addr))
}

/// Set DNS servers on Linux.
///
/// Prefers per-interface tools (`resolvectl`, `nmcli`) that work with
/// systemd-resolved / NetworkManager instead of clobbering the
/// `/etc/resolv.conf` symlink (which is usually managed by resolved). Falls
/// back to writing `/etc/resolv.conf` only when no manager tool exists, and
/// only after validating the inputs as real IPs (no line injection) and
/// requiring root.
pub fn set_dns_servers(primary: &str, secondary: Option<&str>) -> anyhow::Result<()> {
    validate_ip(primary)?;
    if let Some(sec) = secondary {
        validate_ip(sec)?;
    }

    let iface = get_default_interface()?;

    // 1) systemd-resolved: resolvectl dns <iface> <primary> [secondary]
    let mut manager: Option<(&'static str, Vec<String>)> = None;
    if super::common::exec_command("resolvectl", &["--help"]).is_ok() {
        let mut args: Vec<String> = vec!["dns".into(), iface.clone(), primary.to_string()];
        if let Some(sec) = secondary {
            args.push(sec.to_string());
        }
        manager = Some(("resolvectl", args));
    }

    // 2) NetworkManager: nmcli con mod <name> ipv4.dns ...
    if manager.is_none() && super::common::exec_command("nmcli", &["--version"]).is_ok() {
        let mut dns = primary.to_string();
        if let Some(sec) = secondary {
            dns.push(' ');
            dns.push_str(sec);
        }
        manager = Some((
            "nmcli",
            vec![
                "con".to_string(),
                "mod".to_string(),
                iface,
                "ipv4.dns".to_string(),
                dns,
                "ipv4.method".to_string(),
                "auto".to_string(),
            ],
        ));
    }

    if let Some((tool, args)) = manager {
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        // Prepend pkexec when not root (matches GUI usage).
        let out = if check_root_privileges()? {
            std::process::Command::new(tool).args(&arg_refs).output()?
        } else {
            let mut cmd = std::process::Command::new("pkexec");
            cmd.arg(tool).args(&arg_refs);
            cmd.output()?
        };
        if out.status.success() {
            return Ok(());
        }
        return Err(anyhow::anyhow!(
            "failed to set DNS via {}: {}",
            tool,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    // 3) Fallback: direct /etc/resolv.conf write (last resort, needs root).
    if !check_root_privileges()? {
        return Err(anyhow::anyhow!(
            "cannot write /etc/resolv.conf without root; install systemd-resolved or NetworkManager"
        ));
    }

    let resolv_path = std::path::Path::new("/etc/resolv.conf");
    let mut content = String::from("# Generated by NetAssist\n");
    content.push_str(&format!("nameserver {}\n", primary));
    if let Some(secondary) = secondary {
        content.push_str(&format!("nameserver {}\n", secondary));
    }
    std::fs::write(resolv_path, content)?;
    Ok(())
}

/// Read cumulative (rx_bytes, tx_bytes) across all interfaces from
/// `/proc/net/dev`. Columns are (0-indexed): rx_bytes=1, tx_bytes=9.
pub fn get_interface_total_bytes() -> (u64, u64) {
    use std::fs;

    let mut total_rx = 0u64;
    let mut total_tx = 0u64;

    if let Ok(content) = fs::read_to_string("/proc/net/dev") {
        for line in content.lines().skip(2) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 10 {
                // Skip the loopback interface so totals reflect real traffic.
                let name = parts[0].trim_end_matches(':');
                if name == "lo" {
                    continue;
                }
                if let Ok(rx) = parts[1].parse::<u64>() {
                    total_rx += rx;
                }
                if let Ok(tx) = parts[9].parse::<u64>() {
                    total_tx += tx;
                }
            }
        }
    }

    (total_rx, total_tx)
}
