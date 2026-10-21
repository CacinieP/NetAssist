// Linux-specific implementations
//
// The pure parsers live in `platform::common` so they are compiled and
// unit-tested on every host; this file only performs the file/process I/O
// around them.

use super::{ConnectionRawInfo, NetworkInterfaceInfo};
use std::net::IpAddr;

/// Get the default-route gateway on Linux: the lowest-metric IPv4 default
/// route, else the lowest-metric IPv6 one.
pub fn get_default_gateway() -> anyhow::Result<Option<super::DefaultGateway>> {
    for args in [
        &["route", "show", "default"][..],
        &["-6", "route", "show", "default"][..],
    ] {
        if let Ok(output) = super::common::exec_command("ip", args) {
            if let Some(gw) = super::common::parse_ip_route_default(&output) {
                return Ok(Some(gw));
            }
        }
    }
    Ok(None)
}

/// Get default network interface on Linux
pub fn get_default_interface() -> anyhow::Result<String> {
    if let Ok(Some(gw)) = get_default_gateway() {
        if let Some(iface) = gw.interface {
            return Ok(iface);
        }
    }

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
                        if let Some(flags) =
                            line.split_once('<').and_then(|(_, r)| r.split_once('>'))
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

/// Flush DNS cache on Linux.
///
/// Tries `resolvectl`/`systemd-resolve` directly (may need root) and reports
/// real failures instead of returning Ok on a non-zero exit.
pub fn flush_dns_cache() -> anyhow::Result<()> {
    for cmd in ["resolvectl", "systemd-resolve"] {
        let out = std::process::Command::new(cmd)
            .arg("--flush-caches")
            .output();
        if let Ok(out) = out {
            if out.status.success() {
                return Ok(());
            }
            tracing::warn!(
                "{} --flush-caches failed: {}",
                cmd,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
    }
    Err(anyhow::anyhow!(
        "无法刷新 DNS 缓存（需要 systemd-resolved，或需要 root 权限）"
    ))
}

/// Release and renew IP on Linux.
///
/// Uses `nmcli device reapply` (NetworkManager) when available — the old
/// `dhclient -r` released ALL leases and conflicts with NetworkManager.
pub fn release_renew_ip() -> anyhow::Result<()> {
    if super::common::exec_command("nmcli", &["--version"]).is_ok() {
        let iface = get_default_interface()?;
        let out = std::process::Command::new("nmcli")
            .args(["device", "reapply", &iface])
            .output()?;
        if out.status.success() {
            return Ok(());
        }
        return Err(anyhow::anyhow!(
            "nmcli device reapply failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }

    // Fallback for non-NetworkManager systems: dhclient (add -1 so it never
    // blocks for 60s). This is inherently risky on NM systems, hence only a
    // fallback.
    let released = std::process::Command::new("dhclient")
        .arg("-r")
        .arg("-1")
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if released
        && std::process::Command::new("dhclient")
            .arg("-1")
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    {
        return Ok(());
    }
    Err(anyhow::anyhow!(
        "无法续租 IP 地址（需要 NetworkManager 或 dhclient）"
    ))
}

/// Reset network stack on Linux.
///
/// Restarts the network management daemon (needs root — surface the failure
/// instead of returning Ok on a permission error).
pub fn reset_network_stack() -> anyhow::Result<()> {
    let out = std::process::Command::new("systemctl")
        .args(["restart", "NetworkManager"])
        .output()?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    // Non-root is the common GUI case; give a useful message instead of Ok.
    Err(anyhow::anyhow!(
        "重启 NetworkManager 失败（需要 root）: {}",
        stderr
    ))
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

/// Quote `arg` for `/bin/sh` so it is passed through as one literal word.
fn shell_quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', r"'\''"))
}

/// Run each command in order as root, stopping at the first failure: directly
/// when already root, otherwise behind a single `pkexec` prompt.
fn run_as_root(commands: &[Vec<String>]) -> anyhow::Result<()> {
    if check_root_privileges()? {
        for argv in commands {
            let out = std::process::Command::new(&argv[0])
                .args(&argv[1..])
                .output()?;
            if !out.status.success() {
                return Err(anyhow::anyhow!(
                    "{} 失败: {}",
                    argv.join(" "),
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
        }
        return Ok(());
    }

    let script = commands
        .iter()
        .map(|argv| {
            argv.iter()
                .map(|a| shell_quote(a))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join(" && ");
    let out = match std::process::Command::new("pkexec")
        .args(["/bin/sh", "-c", &script])
        .output()
    {
        Ok(out) => out,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::anyhow!(
                "需要 root 权限，但未找到 pkexec：请安装 polkit，或以 root 身份运行 NetAssist"
            ));
        }
        Err(e) => return Err(e.into()),
    };
    match out.status.code() {
        Some(0) => Ok(()),
        Some(126) => Err(anyhow::anyhow!("授权被取消或被拒绝，未做任何修改")),
        Some(127) => Err(anyhow::anyhow!(
            "pkexec 未能完成授权：当前会话可能没有运行 polkit 认证代理（例如无桌面的 SSH 会话）。请在桌面会话中运行，或以 root 身份运行 NetAssist"
        )),
        _ => Err(anyhow::anyhow!(
            "{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

/// Who owns the DNS configuration of an interface.
enum DnsManager {
    /// NetworkManager, with the name of the connection active on the device.
    NetworkManager(String),
    /// systemd-resolved per-link DNS.
    Resolved,
    /// Plain `/etc/resolv.conf`.
    ResolvConf,
}

/// The NetworkManager connection active on `iface`, if NM manages it.
/// `nmcli con mod` needs this connection name, not the device name.
fn nm_connection(iface: &str) -> Option<String> {
    let out = super::common::exec_command(
        "nmcli",
        &["-g", "GENERAL.CONNECTION", "device", "show", iface],
    )
    .ok()?;
    let name = out.trim();
    (!name.is_empty() && name != "--").then(|| name.to_string())
}

fn dns_manager(iface: &str) -> DnsManager {
    if let Some(connection) = nm_connection(iface) {
        DnsManager::NetworkManager(connection)
    } else if super::common::exec_command("resolvectl", &["--help"]).is_ok() {
        DnsManager::Resolved
    } else {
        DnsManager::ResolvConf
    }
}

const RESOLV_CONF: &str = "/etc/resolv.conf";
const RESOLV_CONF_BACKUP: &str = "/etc/resolv.conf.netassist-backup";

/// Poll `read` for up to ~3 s until it reports `addr`; settings applied
/// through NetworkManager or resolved can take a moment to show up.
fn wait_for_dns(addr: &str, read: impl Fn() -> Option<String>) -> bool {
    for attempt in 0..6 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        if read().is_some_and(|out| super::common::dns_list_contains(&out, addr)) {
            return true;
        }
    }
    false
}

/// Set DNS servers on Linux and confirm they took effect.
///
/// NetworkManager-managed devices are changed through their connection
/// profile (persistent, and NM would otherwise overwrite runtime changes);
/// other links go through systemd-resolved, with `~.` as routing domain so
/// all queries use the new servers. `/etc/resolv.conf` is only written as a
/// last resort, after backing up the original for `restore_auto_dns`.
pub fn set_dns_servers(primary: &str, secondary: Option<&str>) -> anyhow::Result<()> {
    validate_ip(primary)?;
    if let Some(sec) = secondary {
        validate_ip(sec)?;
    }
    let servers: Vec<&str> = std::iter::once(primary).chain(secondary).collect();
    let iface = get_default_interface()?;

    match dns_manager(&iface) {
        DnsManager::NetworkManager(connection) => {
            let mut modify = vec![
                "nmcli".to_string(),
                "connection".into(),
                "modify".into(),
                connection,
            ];
            for (family, v6) in [("ipv4", false), ("ipv6", true)] {
                let list: Vec<&str> = servers
                    .iter()
                    .copied()
                    .filter(|s| s.contains(':') == v6)
                    .collect();
                if !list.is_empty() {
                    modify.push(format!("{family}.dns"));
                    modify.push(list.join(" "));
                    modify.push(format!("{family}.ignore-auto-dns"));
                    modify.push("yes".into());
                }
            }
            let reapply = vec![
                "nmcli".to_string(),
                "device".into(),
                "reapply".into(),
                iface.clone(),
            ];
            run_as_root(&[modify, reapply])
                .map_err(|e| anyhow::anyhow!("通过 NetworkManager 设置 DNS 失败: {}", e))?;
            let field = if primary.contains(':') {
                "IP6.DNS"
            } else {
                "IP4.DNS"
            };
            if !wait_for_dns(primary, || {
                super::common::exec_command("nmcli", &["-g", field, "device", "show", &iface]).ok()
            }) {
                return Err(anyhow::anyhow!(
                    "NetworkManager 已接受配置，但 {} 上未生效 {}",
                    iface,
                    primary
                ));
            }
        }
        DnsManager::Resolved => {
            let mut dns = vec!["resolvectl".to_string(), "dns".into(), iface.clone()];
            dns.extend(servers.iter().map(|s| s.to_string()));
            let domain = vec![
                "resolvectl".to_string(),
                "domain".into(),
                iface.clone(),
                "~.".into(),
            ];
            let default_route = vec![
                "resolvectl".to_string(),
                "default-route".into(),
                iface.clone(),
                "true".into(),
            ];
            run_as_root(&[dns, domain, default_route])
                .map_err(|e| anyhow::anyhow!("通过 systemd-resolved 设置 DNS 失败: {}", e))?;
            if !wait_for_dns(primary, || {
                super::common::exec_command("resolvectl", &["dns", &iface]).ok()
            }) {
                return Err(anyhow::anyhow!(
                    "systemd-resolved 已接受配置，但 {} 上未生效 {}",
                    iface,
                    primary
                ));
            }
        }
        DnsManager::ResolvConf => {
            if !check_root_privileges()? {
                return Err(anyhow::anyhow!(
                    "cannot write /etc/resolv.conf without root; install systemd-resolved or NetworkManager"
                ));
            }
            let backup = std::path::Path::new(RESOLV_CONF_BACKUP);
            if !backup.exists() {
                std::fs::write(backup, std::fs::read(RESOLV_CONF).unwrap_or_default())?;
            }
            let mut content = String::from("# Generated by NetAssist\n");
            for server in &servers {
                content.push_str(&format!("nameserver {}\n", server));
            }
            std::fs::write(RESOLV_CONF, content)?;
            let current = get_dns_servers()?.join(" ");
            if !super::common::dns_list_contains(&current, primary) {
                return Err(anyhow::anyhow!(
                    "写入 /etc/resolv.conf 后回读未包含 {}",
                    primary
                ));
            }
        }
    }
    Ok(())
}

/// Return the default interface to the DNS servers its network provides,
/// undoing `set_dns_servers`.
pub fn restore_auto_dns() -> anyhow::Result<()> {
    let iface = get_default_interface()?;
    match dns_manager(&iface) {
        DnsManager::NetworkManager(connection) => {
            let modify = vec![
                "nmcli".to_string(),
                "connection".into(),
                "modify".into(),
                connection.clone(),
                "ipv4.dns".into(),
                String::new(),
                "ipv4.ignore-auto-dns".into(),
                "no".into(),
                "ipv6.dns".into(),
                String::new(),
                "ipv6.ignore-auto-dns".into(),
                "no".into(),
            ];
            let reapply = vec![
                "nmcli".to_string(),
                "device".into(),
                "reapply".into(),
                iface.clone(),
            ];
            run_as_root(&[modify, reapply])
                .map_err(|e| anyhow::anyhow!("通过 NetworkManager 恢复自动 DNS 失败: {}", e))?;
            let left = super::common::exec_command(
                "nmcli",
                &["-g", "ipv4.dns,ipv6.dns", "connection", "show", &connection],
            )?;
            if !left.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "连接 {} 仍配置着静态 DNS（{}），恢复自动 DNS 未生效",
                    connection,
                    left.trim()
                ));
            }
        }
        DnsManager::Resolved => {
            run_as_root(&[vec!["resolvectl".to_string(), "revert".into(), iface]])
                .map_err(|e| anyhow::anyhow!("通过 systemd-resolved 恢复自动 DNS 失败: {}", e))?;
        }
        DnsManager::ResolvConf => {
            let backup = std::path::Path::new(RESOLV_CONF_BACKUP);
            if !backup.exists() {
                return Err(anyhow::anyhow!(
                    "没有找到 NetAssist 修改前的 /etc/resolv.conf 备份，无法自动恢复"
                ));
            }
            if !check_root_privileges()? {
                return Err(anyhow::anyhow!("恢复 /etc/resolv.conf 需要 root 权限"));
            }
            std::fs::write(RESOLV_CONF, std::fs::read(backup)?)?;
            std::fs::remove_file(backup)?;
        }
    }
    Ok(())
}

/// Read a single routed interface, avoiding physical + tunnel double counts.
pub fn get_interface_total_bytes() -> Result<super::CounterSnapshot, String> {
    let interface = [("-4", "1.1.1.1"), ("-6", "2606:4700:4700::1111")]
        .into_iter()
        .find_map(|(family, target)| {
            super::common::exec_command("ip", &[family, "route", "get", target])
                .ok()
                .and_then(|output| super::counters::linux_route_interface(&output))
        })
        .ok_or("Cannot determine an IPv4 or IPv6 routed interface")?;
    let content = std::fs::read_to_string("/proc/net/dev")
        .map_err(|error| format!("Cannot read interface counters: {error}"))?;
    let (rx, tx) = super::counters::linux_interface_bytes(&content, &interface)?;
    Ok(super::CounterSnapshot {
        source: format!("linux:{interface}"),
        rx,
        tx,
    })
}
