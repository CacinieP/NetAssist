//! Pure route/counter parsers. Kept cross-platform for fixture coverage in CI.

pub(super) fn linux_route_interface(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let parts: Vec<_> = line.split_whitespace().collect();
        let index = parts.iter().position(|part| *part == "dev")?;
        parts
            .get(index + 1)
            .filter(|name| **name != "lo")
            .map(|name| name.to_string())
    })
}

pub(super) fn macos_route_interface(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("interface:")
            .map(str::trim)
            .filter(|name| !name.is_empty() && *name != "lo0")
            .map(str::to_string)
    })
}

pub(super) fn linux_interface_bytes(output: &str, interface: &str) -> Result<(u64, u64), String> {
    for line in output.lines() {
        let Some((name, counters)) = line.split_once(':') else {
            continue;
        };
        if name.trim() != interface {
            continue;
        }
        let columns: Vec<_> = counters.split_whitespace().collect();
        if columns.len() < 16 {
            return Err(format!("Incomplete counters for {interface}"));
        }
        let rx = columns[0]
            .parse()
            .map_err(|_| format!("Invalid receive counter for {interface}"))?;
        let tx = columns[8]
            .parse()
            .map_err(|_| format!("Invalid transmit counter for {interface}"))?;
        return Ok((rx, tx));
    }
    Err(format!("No counters for routed interface {interface}"))
}

pub(super) fn macos_interface_bytes(output: &str, interface: &str) -> Result<(u64, u64), String> {
    let mut lines = output.lines();
    let header: Vec<_> = lines
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    let rx_index = header
        .iter()
        .position(|name| *name == "Ibytes")
        .ok_or("Missing Ibytes header")?;
    let tx_index = header
        .iter()
        .position(|name| *name == "Obytes")
        .ok_or("Missing Obytes header")?;
    for line in lines {
        let columns: Vec<_> = line.split_whitespace().collect();
        if columns.first().map(|name| name.trim_end_matches('*')) != Some(interface) {
            continue;
        }
        // Address is blank on a link-only tunnel row. Align the trailing
        // counter columns with the header; never sum link/address duplicates.
        let Some(shift) = header.len().checked_sub(columns.len()) else {
            continue;
        };
        if shift > 1 {
            continue;
        }
        let (Some(rx), Some(tx)) = (rx_index.checked_sub(shift), tx_index.checked_sub(shift))
        else {
            continue;
        };
        if let (Some(rx), Some(tx)) = (columns.get(rx), columns.get(tx)) {
            if let (Ok(rx), Ok(tx)) = (rx.parse(), tx.parse()) {
                return Ok((rx, tx));
            }
        }
    }
    Err(format!(
        "No readable counters for routed interface {interface}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_routes_cover_gateway_point_to_point_and_ipv6() {
        for (output, expected) in [
            (
                "1.1.1.1 via 192.168.1.1 dev wlan0 src 192.168.1.2 uid 1000\n cache",
                "wlan0",
            ),
            ("1.1.1.1 dev tun0 table 100 src 10.0.0.2 uid 1000", "tun0"),
            (
                "2606:4700:4700::1111 from :: via fe80::1 dev eth0 proto ra metric 1024",
                "eth0",
            ),
        ] {
            assert_eq!(linux_route_interface(output).as_deref(), Some(expected));
        }
        assert!(linux_route_interface("RTNETLINK answers: Network is unreachable").is_none());
    }

    #[test]
    fn linux_reads_only_the_selected_route_not_both_tunnel_and_physical() {
        let fixture = "Inter-| Receive | Transmit\nface |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n eth0: 1000000 2 0 0 0 0 0 0 500000 2 0 0 0 0 0 0\n tun0: 900000 2 0 0 0 0 0 0 400000 2 0 0 0 0 0 0\n";
        assert_eq!(
            linux_interface_bytes(fixture, "tun0").unwrap(),
            (900000, 400000)
        );
        assert!(linux_interface_bytes(fixture, "missing0").is_err());
        assert!(linux_interface_bytes("tun0: broken", "tun0").is_err());
    }

    const MAC_HEADER: &str =
        "Name Mtu Network Address Ipkts Ierrs Ibytes Opkts Oerrs Obytes Coll\n";

    #[test]
    fn macos_link_only_tunnel_counters_do_not_require_an_address_column() {
        let fixture = format!("{MAC_HEADER}utun9 1500 <Link#8> 100 0 9000000 200 0 7000000 0\n");
        assert_eq!(
            macos_interface_bytes(&fixture, "utun9").unwrap(),
            (9000000, 7000000)
        );
    }

    #[test]
    fn macos_address_rows_are_not_added_to_the_link_row() {
        let fixture = format!("{MAC_HEADER}en0 1500 <Link#5> 00:11:22:33:44:55 100 0 9000000 200 0 7000000 0\nen0 1500 192.168.1 192.168.1.2 100 - 9000000 200 - 7000000 -\n");
        assert_eq!(
            macos_interface_bytes(&fixture, "en0").unwrap(),
            (9000000, 7000000)
        );
        assert!(macos_interface_bytes(&fixture, "utun9").is_err());
        assert!(macos_interface_bytes(MAC_HEADER, "en0").is_err());
        assert_eq!(
            macos_route_interface("route to: 1.1.1.1\n interface: utun4\n").as_deref(),
            Some("utun4")
        );
    }
}
