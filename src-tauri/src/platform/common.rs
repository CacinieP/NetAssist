// Common utilities available on all platforms

use std::net::IpAddr;

/// Check if IPv6 address is link-local (fe80::/10)
fn is_ipv6_link_local_check(ipv6: &std::net::Ipv6Addr) -> bool {
    let segments = ipv6.segments();
    segments[0] == 0xfe80 && (segments[1] & 0xc000) == 0x8000
}

/// Check if an IP address is local/private
pub fn is_local_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => ipv4.is_loopback() || ipv4.is_private() || ipv4.is_link_local(),
        IpAddr::V6(ipv6) => {
            ipv6.is_loopback() || ipv6.is_unique_local() || is_ipv6_link_local_check(ipv6)
        }
    }
}

/// Classify IP address type
pub fn classify_ip_type(ip: &IpAddr) -> crate::models::IPType {
    match ip {
        IpAddr::V4(ipv4) => {
            if ipv4.is_loopback() || ipv4.is_private() {
                crate::models::IPType::Private
            } else if ipv4.is_link_local() {
                crate::models::IPType::LinkLocal
            } else {
                crate::models::IPType::Public
            }
        }
        IpAddr::V6(ipv6) => {
            if ipv6.is_loopback() || ipv6.is_unique_local() {
                crate::models::IPType::Private
            } else if is_ipv6_link_local_check(ipv6) {
                crate::models::IPType::LinkLocal
            } else {
                crate::models::IPType::Global
            }
        }
    }
}

/// Execute a shell command and return output
#[cfg(unix)]
pub fn exec_command(cmd: &str, args: &[&str]) -> anyhow::Result<String> {
    use std::process::Command;

    let output = Command::new(cmd).args(args).output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(anyhow::anyhow!(
            "Command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

/// Execute a shell command and return output
#[cfg(windows)]
pub fn exec_command(cmd: &str, args: &[&str]) -> anyhow::Result<String> {
    use std::process::Command;

    let output = Command::new(cmd).args(args).output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(anyhow::anyhow!(
            "Command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

// ---------------------------------------------------------------------------
// Linux parsers (moved here so they are compiled + unit-tested on every host;
// linux.rs only performs the file/process I/O around them).
// ---------------------------------------------------------------------------

/// Parse `ss -H -tuanp` output into raw connections.
///
/// Column layout when both tcp+udp are requested:
/// `Netid State Recv-Q Send-Q Local Peer Process`
/// (the `users:(("chrome",pid=1234,fd=45))` suffix may contain spaces, so it
/// is cut off before whitespace-splitting the address columns).
pub fn parse_ss_connections(output: &str) -> Vec<super::ConnectionRawInfo> {
    let mut connections = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Extract process suffix first (may include spaces).
        let mut pid = None;
        let mut process_name = None;
        let head = match line.find("users:((") {
            Some(idx) => {
                let suffix = &line[idx..];
                if let Some(pid_str) = suffix.split("pid=").nth(1) {
                    let pid_num: u32 = pid_str
                        .split([',', ')'])
                        .next()
                        .unwrap_or("0")
                        .trim()
                        .parse()
                        .unwrap_or(0);
                    if pid_num > 0 {
                        pid = Some(pid_num);
                    }
                }
                // process name is inside the first pair of double quotes.
                if let Some(name) = suffix.split('"').nth(1) {
                    process_name = Some(name.to_string());
                }
                &line[..idx]
            }
            None => line,
        };

        let parts: Vec<&str> = head.split_whitespace().collect();
        // ss is always invoked with both -t and -u, so a leading Netid column
        // is present:  Netid State Recv-Q Send-Q Local Peer [Process]
        let protocol = match parts.first() {
            Some(&"tcp") => "TCP",
            Some(&"udp") => "UDP",
            _ => continue,
        };
        if parts.len() < 6 {
            continue;
        }
        let state = normalize_ss_state(parts[1]);
        let local = parts[4];
        let remote = parts[5];

        let Some((local_ip, local_port)) = parse_ss_endpoint(local) else {
            continue;
        };
        let Some((remote_ip, remote_port)) = parse_ss_endpoint(remote) else {
            continue;
        };

        connections.push(super::ConnectionRawInfo {
            protocol: protocol.to_string(),
            local_addr: local_ip,
            local_port,
            remote_addr: remote_ip,
            remote_port,
            state,
            pid,
            process_name,
        });
    }

    connections
}

/// Normalize `ss` state names (TIME-WAIT → TIME_WAIT) to the shared enum.
fn normalize_ss_state(state: &str) -> String {
    state.replace('-', "_").to_uppercase()
}

/// Parse a single ss endpoint: `192.168.1.1:443`, `[2001:db8::1]:53`,
/// `*:5353`, `[::]:*`, `0.0.0.0:*`.
pub fn parse_ss_endpoint(spec: &str) -> Option<(IpAddr, u16)> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }
    // Strip any %zone on link-local IPv6.
    let (ip_part, port_part) = if let Some(rest) = spec.strip_prefix('[') {
        // [v6]:port
        let (ip, port) = rest.split_once("]:")?;
        (ip.to_string(), port.to_string())
    } else {
        let (ip, port) = spec.rsplit_once(':')?;
        (ip.to_string(), port.to_string())
    };
    let ip_part = ip_part.split('%').next().unwrap_or(&ip_part);

    let port: u16 = if port_part == "*" {
        0
    } else {
        port_part.parse().ok()?
    };

    let ip = if ip_part == "*" || ip_part.is_empty() {
        // Wildcard — keep the family of the opposite token unknown; callers
        // only render it, so unspecified v4 is a safe placeholder.
        IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)
    } else {
        ip_part.parse::<IpAddr>().ok()?
    };

    Some((ip, port))
}

/// Map /proc/net/tcp[6] state hex codes to English names.
pub fn proc_state_name(code: &str) -> String {
    match code {
        "01" => "ESTABLISHED".into(),
        "02" => "SYN_SENT".into(),
        "03" => "SYN_RECV".into(),
        "04" => "FIN_WAIT1".into(),
        "05" => "FIN_WAIT2".into(),
        "06" => "TIME_WAIT".into(),
        "07" => "CLOSE".into(),
        "08" => "CLOSE_WAIT".into(),
        "09" => "LAST_ACK".into(),
        "0A" => "LISTEN".into(),
        "0B" => "CLOSING".into(),
        _ => "UNKNOWN".into(),
    }
}

/// Parse one `/proc/net/tcp[6]` line into a connection (no pid available).
pub fn parse_proc_net_line(
    line: &str,
    protocol: &str,
    v6: bool,
) -> Option<super::ConnectionRawInfo> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 10 {
        return None;
    }

    let local_addr = parse_proc_hex_addr(parts[1], v6)?;
    let remote_addr = parse_proc_hex_addr(parts[2], v6)?;

    Some(super::ConnectionRawInfo {
        protocol: protocol.to_string(),
        local_addr: local_addr.0,
        local_port: local_addr.1,
        remote_addr: remote_addr.0,
        remote_port: remote_addr.1,
        state: proc_state_name(parts[3]),
        pid: None,
        process_name: None,
    })
}

/// Parse a `/proc/net/tcp[6]` address token (`0100007F:0050` or 32 hex chars
/// for IPv6). Addresses are printed as little-endian 32-bit words, so byte
/// order must be reversed from a naive `u32::from(hex)`.
pub fn parse_proc_hex_addr(addr: &str, v6: bool) -> Option<(IpAddr, u16)> {
    let parts: Vec<&str> = addr.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let port = u16::from_str_radix(parts[1], 16).ok()?;

    let ip = if v6 {
        let hex = parts[0];
        if hex.len() != 32 {
            return None;
        }
        let mut octets = [0u8; 16];
        // /proc/net/tcp6 stores each 32-bit word little-endian, like tcp.
        for i in 0..4 {
            let word = u32::from_str_radix(&hex[i * 8..(i + 1) * 8], 16).ok()?;
            octets[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        IpAddr::V6(std::net::Ipv6Addr::from(octets))
    } else {
        let word = u32::from_str_radix(parts[0], 16).ok()?;
        // 127.0.0.1 is printed as "0100007F"; the stored value is the raw
        // little-endian bytes of the address.
        IpAddr::V4(std::net::Ipv4Addr::from(word.to_le_bytes()))
    };

    Some((ip, port))
}

fn is_link_local_v6(ip: &std::net::Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
}

/// Build a gateway from a route table entry such as `192.168.1.1`,
/// `fe80::1%en0` or (with `iface`) `fe80::1`.
///
/// Rejects addresses that cannot be a usable next hop: unspecified, a
/// link-local IPv6 address with no known scope, and the bare `fe80::`
/// prefix that tunnel interfaces (utun) list as their "gateway".
pub fn parse_gateway_addr(raw: &str, iface: Option<&str>) -> Option<super::DefaultGateway> {
    let (addr_part, zone) = match raw.split_once('%') {
        Some((addr, zone)) if !zone.is_empty() => (addr, Some(zone.to_string())),
        Some((addr, _)) => (addr, None),
        None => (raw, None),
    };
    let addr: IpAddr = addr_part.parse().ok()?;
    if addr.is_unspecified() {
        return None;
    }
    let iface = iface.filter(|i| !i.is_empty()).map(str::to_string);
    let zone = match addr {
        IpAddr::V6(v6) if is_link_local_v6(&v6) => {
            if v6.segments()[1..].iter().all(|s| *s == 0) {
                return None;
            }
            Some(zone.or_else(|| iface.clone())?)
        }
        _ => None,
    };
    Some(super::DefaultGateway {
        addr,
        zone,
        interface: iface,
    })
}

/// Pick the default gateway among candidates: IPv4 before IPv6, then the
/// lowest route/interface metric; ties keep the original order.
pub fn select_default_gateway(
    mut candidates: Vec<(super::DefaultGateway, u32)>,
) -> Option<super::DefaultGateway> {
    candidates.sort_by_key(|(gw, metric)| (gw.addr.is_ipv6(), *metric));
    candidates.into_iter().next().map(|(gw, _)| gw)
}

/// Parse macOS `route -n get [-inet6] default`:
/// ```text
///     gateway: fe80::1%en1
///   interface: en1
/// ```
pub fn parse_macos_route_get(output: &str) -> Option<super::DefaultGateway> {
    let field = |name: &str| {
        output.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == name).then(|| value.trim().to_string())
        })
    };
    let gateway = field("gateway")?;
    let iface = field("interface");
    parse_gateway_addr(&gateway, iface.as_deref())
}

/// Parse `ip route show default` / `ip -6 route show default` and keep the
/// route with the lowest metric (a missing metric is 0, as in the kernel).
pub fn parse_ip_route_default(output: &str) -> Option<super::DefaultGateway> {
    let candidates = output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.first() != Some(&"default") {
                return None;
            }
            let value_after = |key: &str| {
                parts
                    .iter()
                    .position(|p| *p == key)
                    .and_then(|i| parts.get(i + 1).copied())
            };
            let via = value_after("via")?;
            let dev = value_after("dev");
            let metric = value_after("metric")
                .and_then(|m| m.parse::<u32>().ok())
                .unwrap_or(0);
            Some((parse_gateway_addr(via, dev)?, metric))
        })
        .collect();
    select_default_gateway(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn gateway_keeps_ipv6_zone_and_rejects_unusable_hops() {
        let gw = parse_gateway_addr("fe80::1234%en1", Some("en1")).unwrap();
        assert_eq!(gw.ping_target(), "fe80::1234%en1");
        // Zone taken from the route's interface when the address lacks one.
        assert_eq!(
            parse_gateway_addr("fe80::1", Some("eth0"))
                .unwrap()
                .ping_target(),
            "fe80::1%eth0"
        );
        assert_eq!(parse_gateway_addr("fe80::1", None), None);
        assert_eq!(parse_gateway_addr("fe80::%utun0", Some("utun0")), None);
        assert_eq!(parse_gateway_addr("0.0.0.0", None), None);
        assert_eq!(parse_gateway_addr("link#12", None), None);
        let v4 = parse_gateway_addr("192.168.1.1", Some("en0")).unwrap();
        assert_eq!(v4.ping_target(), "192.168.1.1");
        assert_eq!(v4.interface.as_deref(), Some("en0"));
    }

    #[test]
    fn macos_route_get_reads_gateway_and_interface() {
        let out = "   route to: default\ndestination: default\n       mask: default\n    gateway: 192.168.0.1\n  interface: en1\n      flags: <UP,GATEWAY,DONE,STATIC,PRCLONING,GLOBAL>\n";
        let gw = parse_macos_route_get(out).unwrap();
        assert_eq!(gw.addr, IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1)));
        assert_eq!(gw.interface.as_deref(), Some("en1"));

        let v6 = "    gateway: fe80::d07d:afff:fe7b:d4d2%en1\n  interface: en1\n";
        assert_eq!(
            parse_macos_route_get(v6).unwrap().ping_target(),
            "fe80::d07d:afff:fe7b:d4d2%en1"
        );
        // Tunnel default route without a gateway line.
        assert_eq!(parse_macos_route_get("  interface: utun3\n"), None);
    }

    #[test]
    fn ip_route_picks_lowest_metric() {
        let out = "default via 10.0.0.1 dev wlan0 proto dhcp metric 600\ndefault via 192.168.1.1 dev eth0 proto dhcp metric 100\n";
        let gw = parse_ip_route_default(out).unwrap();
        assert_eq!(gw.addr, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));
        assert_eq!(gw.interface.as_deref(), Some("eth0"));

        let v6 = "default via fe80::1 dev eth0 proto ra metric 1024 pref medium\n";
        assert_eq!(
            parse_ip_route_default(v6).unwrap().ping_target(),
            "fe80::1%eth0"
        );
        assert_eq!(parse_ip_route_default("default dev wg0 scope link\n"), None);
    }

    #[test]
    fn selection_prefers_ipv4_then_metric() {
        let gw = |s: &str| parse_gateway_addr(s, Some("x")).unwrap();
        let picked = select_default_gateway(vec![
            (gw("fe80::1"), 1),
            (gw("10.0.0.1"), 50),
            (gw("192.168.1.1"), 25),
        ])
        .unwrap();
        assert_eq!(picked.addr, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));
        assert_eq!(select_default_gateway(vec![]), None);
    }

    fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(a, b, c, d))
    }

    #[test]
    fn proc_tcp_endianness() {
        // 127.0.0.1:80 → "0100007F:0050"
        let (ip, port) = parse_proc_hex_addr("0100007F:0050", false).unwrap();
        assert_eq!(ip, v4(127, 0, 0, 1));
        assert_eq!(port, 80);
    }

    #[test]
    fn proc_tcp_remote_endianness() {
        // 8.8.8.8:53 → "08080808:0035"
        let (ip, port) = parse_proc_hex_addr("08080808:0035", false).unwrap();
        assert_eq!(ip, v4(8, 8, 8, 8));
        assert_eq!(port, 53);
    }

    #[test]
    fn proc_tcp6_address() {
        // ::1 → 32 chars with the last 32-bit word "01000000" (little-endian)
        let (ip, port) =
            parse_proc_hex_addr("00000000000000000000000001000000:01BB", true).unwrap();
        assert_eq!(ip, IpAddr::V6(Ipv6Addr::LOCALHOST));
        assert_eq!(port, 443);
    }

    #[test]
    fn ss_endpoints() {
        let (ip, port) = parse_ss_endpoint("192.168.1.5:1234").unwrap();
        assert_eq!(ip, v4(192, 168, 1, 5));
        assert_eq!(port, 1234);

        let (ip, port) = parse_ss_endpoint("[2001:db8::1]:53").unwrap();
        assert_eq!(ip, "2001:db8::1".parse::<IpAddr>().unwrap());
        assert_eq!(port, 53);

        let (ip, port) = parse_ss_endpoint("[::]:*").unwrap();
        assert!(ip.is_unspecified());
        assert_eq!(port, 0);

        let (ip, port) = parse_ss_endpoint("0.0.0.0:*").unwrap();
        assert!(ip.is_unspecified());
        assert_eq!(port, 0);
    }

    #[test]
    fn ss_output_parsing_joins_pid() {
        // -tuanp rows: Netid State Recv-Q Send-Q Local Peer users:(("name",...))
        let output = "\
tcp   ESTAB 0      0      192.168.1.5:1234    8.8.8.8:53     users:((\"chrome\",pid=1234,fd=45))
udp   UNCONN 0      0      0.0.0.0:5353       0.0.0.0:*      users:((\"avahi-daemon\",pid=987,fd=12))
tcp   LISTEN 0      128    *:80               *:*            users:((\"nginx\",pid=300,fd=6))
";
        let conns = parse_ss_connections(output);
        assert_eq!(conns.len(), 3);
        let est = conns.iter().find(|c| c.state == "ESTAB").unwrap();
        assert_eq!(est.pid, Some(1234));
        assert_eq!(est.process_name.as_deref(), Some("chrome"));
        let udp = conns.iter().find(|c| c.protocol == "UDP").unwrap();
        assert_eq!(udp.pid, Some(987));
        assert_eq!(udp.local_port, 5353);
        let listen = conns.iter().find(|c| c.state == "LISTEN").unwrap();
        assert_eq!(listen.local_port, 80);
        assert!(listen.local_addr.is_unspecified());
        assert_eq!(listen.pid, Some(300));
    }

    #[test]
    fn proc_state_codes() {
        assert_eq!(proc_state_name("01"), "ESTABLISHED");
        assert_eq!(proc_state_name("0A"), "LISTEN");
        assert_eq!(proc_state_name("06"), "TIME_WAIT");
        assert_eq!(proc_state_name("08"), "CLOSE_WAIT");
    }

    #[test]
    fn proc_net_line_parses() {
        let line = "   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 12345 1 0000000000000000 100 0 0 10 0";
        let conn = parse_proc_net_line(line, "TCP", false).unwrap();
        assert_eq!(conn.local_addr, v4(127, 0, 0, 1));
        assert_eq!(conn.local_port, 8080);
        assert_eq!(conn.state, "LISTEN");
        assert_eq!(conn.remote_port, 0);
    }
}
