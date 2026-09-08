// Windows-specific implementations
#![allow(unused_assignments)]

use super::{ConnectionRawInfo, NetworkInterfaceInfo};
use std::net::IpAddr;
use windows::core::PCSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::NetworkManagement::IpHelper::*;
use windows::Win32::Networking::WinSock::*;
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExA, RegQueryValueExA, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

/// Flags required for FirstGatewayAddress/FirstDnsServerAddress to be filled.
const GAA_FLAGS: GET_ADAPTERS_ADDRESSES_FLAGS = GAA_FLAG_INCLUDE_GATEWAYS;

/// Read the adapter linked list; runs `f` with the head pointer.
///
/// The buffer is allocated as `Vec<u64>` so the pointer is 8-byte aligned,
/// satisfying IP_ADAPTER_ADDRESSES_LH's alignment (avoids the Vec<u8>
/// alignment UB). Handles ERROR_BUFFER_OVERFLOW on both calls by retrying.
fn with_adapters<R>(
    family: u32,
    mut f: impl FnMut(*mut IP_ADAPTER_ADDRESSES_LH) -> R,
) -> Option<R> {
    unsafe {
        for _ in 0..4 {
            let mut size: u32 = 0;
            if GetAdaptersAddresses(family, GAA_FLAGS, None, None, &mut size) != 111 {
                return None;
            }
            // Guard against size == 0 (defensive; never observed in practice).
            let mut buf: Vec<u64> = vec![0; (size.max(1) as usize + 7) / 8];
            let ptr = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
            let rc = GetAdaptersAddresses(family, GAA_FLAGS, None, Some(ptr), &mut size);
            if rc == 0 {
                return Some(f(ptr));
            }
            if rc != 111 {
                return None;
            }
            // Adapters changed between calls; retry with the fresh size.
        }
        None
    }
}

/// Interpret a `sockaddr` pointer as IPv4/IPv6.
unsafe fn sockaddr_to_ip(sa: *const SOCKADDR) -> Option<IpAddr> {
    if sa.is_null() {
        return None;
    }
    let family = (*sa).sa_family.0 as i32;
    if family == AF_INET.0 as i32 {
        let s4 = &*(sa as *const SOCKADDR_IN);
        let bytes = s4.sin_addr.S_un.S_addr.to_le_bytes();
        Some(IpAddr::V4(std::net::Ipv4Addr::new(
            bytes[0], bytes[1], bytes[2], bytes[3],
        )))
    } else if family == AF_INET6.0 as i32 {
        let s6 = &*(sa as *const SOCKADDR_IN6);
        let bytes = &s6.sin6_addr.u.Byte;
        Some(IpAddr::V6(std::net::Ipv6Addr::from(*bytes)))
    } else {
        None
    }
}

/// Convert a NUL-terminated UTF-16 string to a Rust String.
unsafe fn wide_to_string(mut ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *ptr != 0 {
        len += 1;
        ptr = ptr.add(1);
    }
    if len == 0 {
        return String::new();
    }
    let slice = std::slice::from_raw_parts(ptr.sub(len), len);
    String::from_utf16_lossy(slice)
}

/// Convert an ANSI (char*) string to a Rust String (interface GUIDs are pure
/// ASCII, so from_utf8 is safe).
unsafe fn ansi_to_string(mut ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *ptr != 0 {
        len += 1;
        ptr = ptr.add(1);
    }
    if len == 0 {
        return String::new();
    }
    let bytes = std::slice::from_raw_parts(ptr.sub(len), len);
    String::from_utf8_lossy(bytes).into_owned()
}

/// Get default gateway on Windows (the default-route gateway of the first
/// usable adapter).
pub fn get_default_gateway() -> anyhow::Result<Option<IpAddr>> {
    let mut found: Option<IpAddr> = None;
    with_adapters(AF_INET.0 as u32, |head| {
        let mut cur = head;
        while !cur.is_null() {
            let a = unsafe { &*cur };
            // Only operational adapters are considered.
            if a.OperStatus.0 == 1 {
                let mut gw = a.FirstGatewayAddress;
                while !gw.is_null() {
                    let g = unsafe { &*gw };
                    if let Some(ip) = unsafe { sockaddr_to_ip(g.Address.lpSockaddr) } {
                        found = Some(ip);
                        return;
                    }
                    gw = g.Next;
                }
            }
            cur = unsafe { (*cur).Next };
        }
    });
    Ok(found)
}

/// Get the friendly name of the default network interface.
pub fn get_default_interface() -> anyhow::Result<String> {
    let mut chosen: Option<String> = None;
    with_adapters(AF_INET.0 as u32, |head| {
        let mut cur = head;
        while !cur.is_null() {
            let a = unsafe { &*cur };
            if a.OperStatus.0 == 1 && !a.FirstGatewayAddress.is_null() {
                chosen = Some(unsafe { wide_to_string(a.FriendlyName.0) });
                return;
            }
            cur = unsafe { (*cur).Next };
        }
    });

    // Fall back to the first operational non-loopback adapter with an address.
    if chosen.is_none() {
        with_adapters(0, |head| {
            let mut cur = head;
            while !cur.is_null() {
                let a = unsafe { &*cur };
                if a.OperStatus.0 == 1
                    && a.IfType != 24
                    && !a.FirstUnicastAddress.is_null()
                {
                    chosen = Some(unsafe { wide_to_string(a.FriendlyName.0) });
                    return;
                }
                cur = unsafe { (*cur).Next };
            }
        });
    }

    Ok(chosen.unwrap_or_else(|| "Ethernet".to_string()))
}

/// Get network interfaces on Windows
pub fn get_network_interfaces() -> anyhow::Result<Vec<NetworkInterfaceInfo>> {
    let mut interfaces = Vec::new();
    with_adapters(0, |head| {
        let mut cur = head;
        while !cur.is_null() {
            let a = unsafe { &*cur };
            let name = unsafe { wide_to_string(a.FriendlyName.0) };
            let description = if !a.Description.is_null() {
                unsafe { wide_to_string(a.Description.0) }
            } else {
                name.clone()
            };

            let mut ipv4_addrs = Vec::new();
            let mut ipv6_addrs = Vec::new();
            let mut unicast = a.FirstUnicastAddress;
            while !unicast.is_null() {
                let u = unsafe { &*unicast };
                if let Some(ip) = unsafe { sockaddr_to_ip(u.Address.lpSockaddr) } {
                    match ip {
                        IpAddr::V4(_) => ipv4_addrs.push(ip),
                        IpAddr::V6(_) => ipv6_addrs.push(ip),
                    }
                }
                unicast = u.Next;
            }

            let is_up = a.OperStatus.0 == 1; // IfOperStatusUp
            let is_loopback = a.IfType == 24; // IF_TYPE_SOFTWARE_LOOPBACK

            if is_up && !is_loopback && (!ipv4_addrs.is_empty() || !ipv6_addrs.is_empty()) {
                interfaces.push(NetworkInterfaceInfo {
                    name: name.clone(),
                    display_name: description,
                    ipv4_addresses: ipv4_addrs.clone(),
                    ipv6_addresses: ipv6_addrs.clone(),
                    is_up,
                    is_loopback,
                    gateway: None,
                });
            }
            cur = unsafe { (*cur).Next };
        }
    });
    Ok(interfaces)
}

/// Map Windows MIB_TCP_STATE values (1=CLOSED … 12=DELETE_TCB) to names.
fn tcp_state_name(state: u32) -> &'static str {
    match state {
        1 => "CLOSED",
        2 => "LISTEN",
        3 => "SYN_SENT",
        4 => "SYN_RECV",
        5 => "ESTABLISHED",
        6 => "FIN_WAIT1",
        7 => "FIN_WAIT2",
        8 => "CLOSE_WAIT",
        9 => "CLOSING",
        10 => "LAST_ACK",
        11 => "TIME_WAIT",
        12 => "DELETE_TCB",
        _ => "UNKNOWN",
    }
}

/// TCP/IP ports in MIB tables are stored network byte order in the low 16
/// bits of the DWORD field.
fn mib_port(dw: u32) -> u16 {
    let raw = dw as u16;
    raw.swap_bytes()
}

/// Get active connections on Windows using GetExtendedTcpTable /
/// GetExtendedUdpTable.
pub fn get_active_connections() -> anyhow::Result<Vec<ConnectionRawInfo>> {
    unsafe {
        let mut connections = Vec::new();
        let mut pids = std::collections::HashSet::new();

        // --- TCP (IPv4) ---
        let mut size = 0u32;
        let mut rc = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );
        if rc == 122 {
            let mut buf: Vec<u64> = vec![0; (size as usize + 7) / 8];
            let table = buf.as_mut_ptr() as *mut MIB_TCPTABLE_OWNER_PID;
            rc = GetExtendedTcpTable(
                Some(table as *mut _),
                &mut size,
                false,
                AF_INET.0 as u32,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            );
            if rc == 0 {
                let t = &*table;
                for i in 0..t.dwNumEntries as usize {
                    let row = &*t.table.as_ptr().add(i);
                    let lb = row.dwLocalAddr.to_le_bytes();
                    let rb = row.dwRemoteAddr.to_le_bytes();
                    pids.insert(row.dwOwningPid);
                    connections.push(ConnectionRawInfo {
                        protocol: "TCP".to_string(),
                        local_addr: IpAddr::V4(std::net::Ipv4Addr::new(
                            lb[0], lb[1], lb[2], lb[3],
                        )),
                        local_port: mib_port(row.dwLocalPort),
                        remote_addr: IpAddr::V4(std::net::Ipv4Addr::new(
                            rb[0], rb[1], rb[2], rb[3],
                        )),
                        remote_port: mib_port(row.dwRemotePort),
                        state: tcp_state_name(row.dwState).to_string(),
                        pid: Some(row.dwOwningPid),
                        process_name: None,
                    });
                }
            }
        }

        // --- UDP (IPv4) ---
        let mut size = 0u32;
        let mut rc = GetExtendedUdpTable(
            None,
            &mut size,
            false,
            AF_INET.0 as u32,
            UDP_TABLE_OWNER_PID,
            0,
        );
        if rc == 122 {
            let mut buf: Vec<u64> = vec![0; (size as usize + 7) / 8];
            let table = buf.as_mut_ptr() as *mut MIB_UDPTABLE_OWNER_PID;
            rc = GetExtendedUdpTable(
                Some(table as *mut _),
                &mut size,
                false,
                AF_INET.0 as u32,
                UDP_TABLE_OWNER_PID,
                0,
            );
            if rc == 0 {
                let t = &*table;
                for i in 0..t.dwNumEntries as usize {
                    let row = &*t.table.as_ptr().add(i);
                    let lb = row.dwLocalAddr.to_le_bytes();
                    pids.insert(row.dwOwningPid);
                    connections.push(ConnectionRawInfo {
                        protocol: "UDP".to_string(),
                        local_addr: IpAddr::V4(std::net::Ipv4Addr::new(
                            lb[0], lb[1], lb[2], lb[3],
                        )),
                        local_port: mib_port(row.dwLocalPort),
                        remote_addr: IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
                        remote_port: 0,
                        state: "ACTIVE".to_string(),
                        pid: Some(row.dwOwningPid),
                        process_name: None,
                    });
                }
            }
        }

        let process_names = get_process_names_batch(&pids);
        for conn in &mut connections {
            if let Some(pid) = conn.pid {
                conn.process_name = process_names.get(&pid).cloned();
            }
        }

        Ok(connections)
    }
}

/// Get process names for multiple PIDs using Windows native APIs (public for traffic module)
pub fn get_process_names_batch(
    pids: &std::collections::HashSet<u32>,
) -> std::collections::HashMap<u32, String> {
    use std::collections::HashMap;

    let mut result = HashMap::new();
    if pids.is_empty() {
        return result;
    }

    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) => h,
            Err(e) => {
                tracing::error!("Failed to create process snapshot: {}", e);
                for &pid in pids {
                    result.insert(pid, "-".to_string());
                }
                return result;
            }
        };

        // Wide-char snapshot so Unicode process names survive regardless of
        // the ANSI code page.
        let mut pid_to_exe: HashMap<u32, String> = HashMap::new();
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let name = wide_to_string(entry.szExeFile.as_ptr());
                pid_to_exe.insert(entry.th32ProcessID, name);
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);

        for &pid in pids {
            if let Some(name) = try_get_process_image_name(pid) {
                result.insert(pid, name);
            } else if let Some(exe_name) = pid_to_exe.get(&pid) {
                result.insert(pid, exe_name.clone());
            } else {
                result.insert(pid, "-".to_string());
            }
        }
    }
    result
}

/// Try to get the full process image name using OpenProcess and QueryFullProcessImageNameW
fn try_get_process_image_name(pid: u32) -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::core::PWSTR;

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
            .or_else(|_| OpenProcess(PROCESS_QUERY_INFORMATION, false, pid))
            .ok()?;

        let mut buffer = [0u16; 520]; // MAX_PATH * 2
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);

        if ok.is_ok() && size > 0 {
            let full_path = OsString::from_wide(&buffer[..size as usize])
                .to_string_lossy()
                .into_owned();
            return Some(
                full_path
                    .split('\\')
                    .last()
                    .filter(|f| !f.is_empty())
                    .map(str::to_string)
                    .unwrap_or(full_path),
            );
        }
        None
    }
}

/// Get process name from PID using Windows native APIs
fn get_process_name(pid: u32) -> Option<String> {
    let mut pids = std::collections::HashSet::new();
    pids.insert(pid);
    let mut result = get_process_names_batch(&pids);
    result.remove(&pid)
}

/// Flush DNS cache on Windows
pub fn flush_dns_cache() -> anyhow::Result<()> {
    let output = std::process::Command::new("ipconfig")
        .args(&["/flushdns"])
        .output()?;

    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Failed to flush DNS cache: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

/// Release and renew IP on Windows
pub fn release_renew_ip() -> anyhow::Result<()> {
    std::process::Command::new("ipconfig")
        .args(&["/release"])
        .output()?;

    std::process::Command::new("ipconfig")
        .args(&["/renew"])
        .output()?;

    Ok(())
}

/// Reset network stack on Windows
pub fn reset_network_stack() -> anyhow::Result<()> {
    std::process::Command::new("netsh")
        .args(&["winsock", "reset"])
        .output()?;

    std::process::Command::new("netsh")
        .args(&["int", "ip", "reset"])
        .output()?;

    Ok(())
}

/// Get DNS servers on Windows.
///
/// Reads the DNS configuration from the registry for the active interface's
/// GUID (static `NameServer` or DHCP-provided `DhcpNameServer`). Returns an
/// empty list (never fake data) when nothing can be determined.
pub fn get_dns_servers() -> anyhow::Result<Vec<String>> {
    // Discover active adapters and read their registry DNS values.
    let mut servers: Vec<String> = Vec::new();
    let mut guids: Vec<String> = Vec::new();
    with_adapters(0, |head| {
        let mut cur = head;
        while !cur.is_null() {
            let a = unsafe { &*cur };
            if a.OperStatus.0 == 1 && !a.AdapterName.is_null() {
                guids.push(unsafe { ansi_to_string(a.AdapterName.0) });
            }
            cur = unsafe { (*cur).Next };
        }
    });

    let base = "SYSTEM\\CurrentControlSet\\Services\\Tcpip\\Parameters\\Interfaces\\";
    for guid in &guids {
        if guid.is_empty() {
            continue;
        }
        let path = format!("{}{}\\0", base, guid);
        unsafe {
            let mut hkey = HKEY::default();
            if RegOpenKeyExA(
                HKEY_LOCAL_MACHINE,
                PCSTR(path.as_ptr()),
                0,
                KEY_READ,
                &mut hkey,
            )
            .0
                == 0
            {
                for value_name in ["NameServer\0", "DhcpNameServer\0"] {
                    let mut buf = [0u8; 2048];
                    let mut size = buf.len() as u32;
                    let mut ty = windows::Win32::System::Registry::REG_VALUE_TYPE::default();
                    let rc = RegQueryValueExA(
                        hkey,
                        PCSTR(value_name.as_ptr()),
                        None,
                        Some(&mut ty),
                        Some(buf.as_mut_ptr()),
                        Some(&mut size),
                    );
                    if rc.0 == 0 && size > 0 {
                        let s = String::from_utf8_lossy(&buf[..size as usize])
                            .trim_matches('\0')
                            .trim()
                            .to_string();
                        for part in s.split([',', ' ']) {
                            let part = part.trim();
                            if !part.is_empty()
                                && part.parse::<IpAddr>().is_ok()
                                && !servers.contains(&part.to_string())
                            {
                                servers.push(part.to_string());
                            }
                        }
                    }
                }
                let _ = RegCloseKey(hkey);
            }
        }
    }

    Ok(servers)
}

/// Validate interface name to prevent command injection
fn validate_interface_name(name: &str) -> Result<(), anyhow::Error> {
    if name.is_empty() || name.len() > 100 {
        return Err(anyhow::anyhow!("Invalid interface name length"));
    }
    let dangerous_chars = [
        '&', '|', ';', '$', '`', '(', ')', '<', '>', '\0', '\n', '\r', '\t', '"', '\'',
    ];
    if name.chars().any(|c| dangerous_chars.contains(&c)) {
        return Err(anyhow::anyhow!(
            "Interface name contains dangerous characters"
        ));
    }
    Ok(())
}

/// Set DNS servers on Windows using netsh with proper input validation.
///
/// The active interface is found by parsing `netsh interface show interface`
/// correctly: columns are `Admin | State | Type | Interface Name`, and the
/// name is everything after the first three columns (may contain spaces).
pub fn set_dns_servers(primary: &str, secondary: Option<&str>) -> anyhow::Result<()> {
    if primary.parse::<IpAddr>().is_err() {
        return Err(anyhow::anyhow!("Invalid primary DNS server address"));
    }
    if let Some(sec) = secondary {
        if sec.parse::<IpAddr>().is_err() {
            return Err(anyhow::anyhow!("Invalid secondary DNS server address"));
        }
    }

    // Get connected interface name from netsh.
    let output = std::process::Command::new("netsh")
        .args(&["interface", "show", "interface"])
        .output()?;
    let content = String::from_utf8_lossy(&output.stdout);

    let mut interface_name: Option<String> = None;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("Admin State")
            || trimmed.starts_with('-')
        {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 4
            && parts.get(1).map(|s| s.eq_ignore_ascii_case("Connected")).unwrap_or(false)
        {
            interface_name = Some(parts[3..].join(" "));
            break;
        }
    }

    let interface_name = interface_name
        .ok_or_else(|| anyhow::anyhow!("no connected network interface found"))?;
    validate_interface_name(&interface_name)?;

    let name_arg = format!("name={}", interface_name);

    let set = std::process::Command::new("netsh")
        .args(&[
            "interface",
            "ip",
            "set",
            "dns",
            &name_arg,
            "static",
            primary,
        ])
        .status()?;
    if !set.success() {
        return Err(anyhow::anyhow!(
            "failed to set primary DNS (admin rights required?)"
        ));
    }

    if let Some(secondary) = secondary {
        let add = std::process::Command::new("netsh")
            .args(&[
                "interface",
                "ip",
                "add",
                "dns",
                &name_arg,
                secondary,
                "index=2",
            ])
            .status()?;
        if !add.success() {
            return Err(anyhow::anyhow!(
                "failed to set secondary DNS (admin rights required?)"
            ));
        }
    }

    Ok(())
}

/// Kill a TCP connection on Windows by resetting it with SetTcpEntry
/// (MIB_TCP_STATE_DELETE_TCB), falling back to terminating the owning process.
pub fn kill_connection(
    local_addr: IpAddr,
    local_port: u16,
    remote_addr: IpAddr,
    remote_port: u16,
) -> anyhow::Result<bool> {
    let (IpAddr::V4(local_v4), IpAddr::V4(remote_v4)) = (local_addr, remote_addr) else {
        return Ok(false);
    };

    unsafe {
        let mut size = 0u32;
        let mut rc = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );
        if rc == 122 {
            let mut buf: Vec<u64> = vec![0; (size as usize + 7) / 8];
            let table = buf.as_mut_ptr() as *mut MIB_TCPTABLE_OWNER_PID;
            rc = GetExtendedTcpTable(
                Some(table as *mut _),
                &mut size,
                false,
                AF_INET.0 as u32,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            );
            if rc == 0 {
                let t = &*table;
                for i in 0..t.dwNumEntries as usize {
                    let row = &*t.table.as_ptr().add(i);
                    let lb = row.dwLocalAddr.to_le_bytes();
                    let rb = row.dwRemoteAddr.to_le_bytes();
                    if IpAddr::V4(std::net::Ipv4Addr::new(lb[0], lb[1], lb[2], lb[3]))
                        == IpAddr::V4(local_v4)
                        && IpAddr::V4(std::net::Ipv4Addr::new(rb[0], rb[1], rb[2], rb[3]))
                            == IpAddr::V4(remote_v4)
                        && mib_port(row.dwLocalPort) == local_port
                        && mib_port(row.dwRemotePort) == remote_port
                    {
                        // Prefer resetting just this connection.
                        let mut tcp_row = MIB_TCPROW_LH {
                            Anonymous: MIB_TCPROW_LH_0 {
                                dwState: MIB_TCP_STATE_DELETE_TCB.0 as u32,
                            },
                            dwLocalAddr: row.dwLocalAddr,
                            dwLocalPort: row.dwLocalPort,
                            dwRemoteAddr: row.dwRemoteAddr,
                            dwRemotePort: row.dwRemotePort,
                        };
                        if SetTcpEntry(&mut tcp_row) == 0 {
                            return Ok(true);
                        }
                        // Fallback: kill the owning process.
                        let pid = row.dwOwningPid;
                        let out = std::process::Command::new("taskkill")
                            .args(["/PID", &pid.to_string(), "/F"])
                            .output()?;
                        return Ok(out.status.success());
                    }
                }
            }
        }
    }
    Ok(false)
}

/// Read cumulative (rx_bytes, tx_bytes) across all operational interfaces via
/// `GetIfTable2`. Excludes loopback; VPN/tunnel interfaces are excluded by
/// the caller (traffic.rs) so totals match user-visible traffic.
pub fn get_interface_total_bytes() -> (u64, u64) {
    unsafe {
        let mut if_table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        let result = GetIfTable2(&mut if_table);

        if result.0 == 0 && !if_table.is_null() {
            let table = &*if_table;
            let mut total_rx = 0u64;
            let mut total_tx = 0u64;

            let num_entries = table.NumEntries as usize;
            let table_ptr = table.Table.as_ptr();

            for i in 0..num_entries {
                let row = &*table_ptr.add(i);
                if row.OperStatus.0 == 1 && row.Type != IF_TYPE_SOFTWARE_LOOPBACK {
                    total_rx += row.InOctets;
                    total_tx += row.OutOctets;
                }
            }

            FreeMibTable(if_table as *mut _);
            (total_rx, total_tx)
        } else {
            (0, 0)
        }
    }
}
