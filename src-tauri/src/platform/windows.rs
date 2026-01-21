// Windows-specific implementations
#![allow(unused_assignments)]

use super::{NetworkInterfaceInfo, ConnectionRawInfo};
use std::net::IpAddr;
use windows::Win32::NetworkManagement::IpHelper::*;
use windows::Win32::Networking::WinSock::*;
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RegOpenKeyExA, RegCloseKey, KEY_READ};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_QUERY_INFORMATION, QueryFullProcessImageNameW};
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::Foundation::CloseHandle;
use windows::core::PCSTR;

/// Get default gateway on Windows
pub fn get_default_gateway() -> anyhow::Result<Option<IpAddr>> {
    use windows::Win32::NetworkManagement::IpHelper::GET_ADAPTERS_ADDRESSES_FLAGS;

    unsafe {
        let mut adapter_addresses: *mut IP_ADAPTER_ADDRESSES_LH = std::ptr::null_mut();
        let mut size = 0;

        // First call to get the buffer size needed
        let result = GetAdaptersAddresses(
            AF_INET.0 as u32,
            GET_ADAPTERS_ADDRESSES_FLAGS(0),
            None,
            None,
            &mut size,
        );

        if result != 111 { // ERROR_BUFFER_OVERFLOW
            return Ok(None);
        }

        // Allocate buffer using Vec
        let mut buffer = vec![0u8; size as usize];
        adapter_addresses = buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;

        // Second call to get the actual data
        let result = GetAdaptersAddresses(
            AF_INET.0 as u32,
            GET_ADAPTERS_ADDRESSES_FLAGS(0),
            None,
            Some(adapter_addresses),
            &mut size,
        );

        if result != 0 { // NO_ERROR is 0
            return Ok(None);
        }

        // Iterate through adapters to find default gateway
        let mut current = adapter_addresses;
        while !current.is_null() {
            let adapter = &*current;

            // Check first gateway address
            let mut gateway = adapter.FirstGatewayAddress;
            while !gateway.is_null() {
                let addr = &*gateway;
                let sockaddr = &*addr.Address.lpSockaddr;

                let family = sockaddr.sa_family.0 as i32;
                if family == AF_INET.0 as i32 {
                    let inet_sockaddr = &*(addr.Address.lpSockaddr as *const _ as *const SOCKADDR_IN);
                    let ip_bytes = inet_sockaddr.sin_addr.S_un.S_addr.to_ne_bytes();
                    let ip = IpAddr::V4(std::net::Ipv4Addr::new(ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3]));
                    return Ok(Some(ip));
                }

                gateway = addr.Next;
            }

            current = adapter.Next;
        }

        Ok(None)
    }
}

/// Get default network interface on Windows
pub fn get_default_interface() -> anyhow::Result<String> {
    use windows::Win32::NetworkManagement::IpHelper::GET_ADAPTERS_ADDRESSES_FLAGS;

    unsafe {
        let mut adapter_addresses: *mut IP_ADAPTER_ADDRESSES_LH = std::ptr::null_mut();
        let mut size = 0;

        // First call to get the buffer size needed
        let result = GetAdaptersAddresses(
            AF_INET.0 as u32,
            GET_ADAPTERS_ADDRESSES_FLAGS(0),
            None,
            None,
            &mut size,
        );

        if result != 111 { // ERROR_BUFFER_OVERFLOW
            return Ok("Ethernet0".to_string());
        }

        // Allocate buffer using Vec
        let mut buffer = vec![0u8; size as usize];
        adapter_addresses = buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;

        // Second call to get the actual data
        let result = GetAdaptersAddresses(
            AF_INET.0 as u32,
            GET_ADAPTERS_ADDRESSES_FLAGS(0),
            None,
            Some(adapter_addresses),
            &mut size,
        );

        if result != 0 { // NO_ERROR is 0
            return Ok("Ethernet0".to_string());
        }

        // Return first active adapter
        let mut current = adapter_addresses;
        while !current.is_null() {
            let adapter = &*current;

            // Convert FriendlyName to String
            let name = String::from_utf16_lossy(
                std::slice::from_raw_parts(adapter.FriendlyName.as_ptr(), adapter.FriendlyName.len())
            );

            // Check if adapter is operational and has a gateway
            let gateway = adapter.FirstGatewayAddress;
            if !gateway.is_null() {
                return Ok(name);
            }

            current = adapter.Next;
        }

        Ok("Ethernet0".to_string())
    }
}

/// Get network interfaces on Windows
pub fn get_network_interfaces() -> anyhow::Result<Vec<NetworkInterfaceInfo>> {
    use windows::Win32::NetworkManagement::IpHelper::GET_ADAPTERS_ADDRESSES_FLAGS;

    unsafe {
        let mut adapter_addresses: *mut IP_ADAPTER_ADDRESSES_LH = std::ptr::null_mut();
        let mut size = 0;

        // First call to get the buffer size needed
        let result = GetAdaptersAddresses(
            0, // AF_UNSPEC
            GET_ADAPTERS_ADDRESSES_FLAGS(0),
            None,
            None,
            &mut size,
        );

        if result != 111 { // ERROR_BUFFER_OVERFLOW
            return Ok(vec![]);
        }

        // Allocate buffer using Vec
        let mut buffer = vec![0u8; size as usize];
        adapter_addresses = buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;

        // Second call to get the actual data
        let result = GetAdaptersAddresses(
            0,
            GET_ADAPTERS_ADDRESSES_FLAGS(0),
            None,
            Some(adapter_addresses),
            &mut size,
        );

        if result != 0 { // NO_ERROR is 0
            return Ok(vec![]);
        }

        let mut interfaces = Vec::new();
        let mut current = adapter_addresses;

        while !current.is_null() {
            let adapter = &*current;

            // Get adapter name from FriendlyName
            let name = if !adapter.FriendlyName.is_null() {
                // PWSTR is a wide string pointer
                let mut len = 0;
                let mut ptr = adapter.FriendlyName.0;
                while *ptr != 0 {
                    len += 1;
                    ptr = ptr.offset(1);
                }
                let slice = std::slice::from_raw_parts(adapter.FriendlyName.0, len);
                String::from_utf16_lossy(slice)
            } else {
                "Unknown Adapter".to_string()
            };

            // Get description - it's a PWSTR (wide string) not a regular string
            let description = if !adapter.Description.is_null() {
                let mut len = 0;
                let mut ptr = adapter.Description.0;
                while *ptr != 0 {
                    len += 1;
                    ptr = ptr.offset(1);
                }
                let slice = std::slice::from_raw_parts(adapter.Description.0, len);
                String::from_utf16_lossy(slice)
            } else {
                name.clone()
            };

            // Collect IPv4 and IPv6 addresses
            let mut ipv4_addrs = Vec::new();
            let mut ipv6_addrs = Vec::new();

            let mut unicast = adapter.FirstUnicastAddress;
            while !unicast.is_null() {
                let addr = &*unicast;
                let sockaddr = &*addr.Address.lpSockaddr;

                // Check address family using the raw value
                let family = sockaddr.sa_family.0 as i32;

                if family == AF_INET.0 as i32 {
                    let inet_sockaddr = &*(addr.Address.lpSockaddr as *const _ as *const SOCKADDR_IN);
                    let ip_bytes = inet_sockaddr.sin_addr.S_un.S_addr.to_ne_bytes();
                    let ip = IpAddr::V4(std::net::Ipv4Addr::new(ip_bytes[0], ip_bytes[1], ip_bytes[2], ip_bytes[3]));
                    ipv4_addrs.push(ip);
                } else if family == AF_INET6.0 as i32 {
                    let inet6_sockaddr = &*(addr.Address.lpSockaddr as *const _ as *const SOCKADDR_IN6);
                    // Access IPv6 bytes through the u field which is a union
                    let ip_bytes = std::slice::from_raw_parts(&inet6_sockaddr.sin6_addr.u.Byte as *const u8, 16);
                    let ip = IpAddr::V6(std::net::Ipv6Addr::new(
                        u16::from_be_bytes([ip_bytes[0], ip_bytes[1]]),
                        u16::from_be_bytes([ip_bytes[2], ip_bytes[3]]),
                        u16::from_be_bytes([ip_bytes[4], ip_bytes[5]]),
                        u16::from_be_bytes([ip_bytes[6], ip_bytes[7]]),
                        u16::from_be_bytes([ip_bytes[8], ip_bytes[9]]),
                        u16::from_be_bytes([ip_bytes[10], ip_bytes[11]]),
                        u16::from_be_bytes([ip_bytes[12], ip_bytes[13]]),
                        u16::from_be_bytes([ip_bytes[14], ip_bytes[15]]),
                    ));
                    ipv6_addrs.push(ip);
                }

                unicast = addr.Next;
            }

            // Check if interface is up
            let is_up = adapter.OperStatus.0 == 1; // IfOperStatusUp

            // Check if loopback
            let is_loopback = adapter.IfType == 24; // IF_TYPE_SOFTWARE_LOOPBACK

            if is_up && !is_loopback && (!ipv4_addrs.is_empty() || !ipv6_addrs.is_empty()) {
                interfaces.push(NetworkInterfaceInfo {
                    name: name.clone(),
                    display_name: description,
                    ipv4_addresses: ipv4_addrs.clone(),
                    ipv6_addresses: ipv6_addrs.clone(),
                    is_up,
                    is_loopback,
                    gateway: None, // Gateway can be populated separately
                });
            }

            current = adapter.Next;
        }

        Ok(interfaces)
    }
}

/// Get active connections on Windows using GetExtendedTcpTable
pub fn get_active_connections() -> anyhow::Result<Vec<ConnectionRawInfo>> {
    unsafe {
        let mut connections = Vec::new();
        let mut pids = std::collections::HashSet::new();

        // Get TCP table
        let mut tcp_table: *mut MIB_TCPTABLE_OWNER_PID = std::ptr::null_mut();
        let mut size = 0;

        let result = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );

        if result == 122 { // ERROR_INSUFFICIENT_BUFFER
            let mut buffer = vec![0u8; size as usize];
            tcp_table = buffer.as_mut_ptr() as *mut MIB_TCPTABLE_OWNER_PID;

            let result = GetExtendedTcpTable(
                Some(tcp_table as *mut _),
                &mut size,
                false,
                AF_INET.0 as u32,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            );

            if result == 0 && !tcp_table.is_null() {
                let table = &*tcp_table;
                let num_entries = table.dwNumEntries as usize;

                for i in 0..num_entries {
                    let row_ptr = table.table.as_ptr().add(i);
                    let row = &*row_ptr;
                    // Windows stores IP addresses as DWORD in network byte order
                    // Ipv4Addr::from(u32) expects network byte order, but on little-endian systems
                    // we need to convert because DWORD is stored in host byte order
                    let local_ip = IpAddr::V4(std::net::Ipv4Addr::from(row.dwLocalAddr.to_be()));
                    let local_port = (row.dwLocalPort as u16 >> 8) | ((row.dwLocalPort as u16 & 0xFF) << 8);
                    let remote_ip = IpAddr::V4(std::net::Ipv4Addr::from(row.dwRemoteAddr.to_be()));
                    let remote_port = (row.dwRemotePort as u16 >> 8) | ((row.dwRemotePort as u16 & 0xFF) << 8);
                    let state = match row.dwState {
                        1 => "ESTABLISHED",
                        2 => "SYN_SENT",
                        3 => "SYN_RECV",
                        4 => "FIN_WAIT1",
                        5 => "FIN_WAIT2",
                        6 => "TIME_WAIT",
                        7 => "CLOSE",
                        8 => "CLOSE_WAIT",
                        9 => "LAST_ACK",
                        10 => "LISTEN",
                        11 => "CLOSING",
                        _ => "UNKNOWN",
                    };

                    pids.insert(row.dwOwningPid);
                    connections.push(ConnectionRawInfo {
                        protocol: "TCP".to_string(),
                        local_addr: local_ip,
                        local_port,
                        remote_addr: remote_ip,
                        remote_port,
                        state: state.to_string(),
                        pid: Some(row.dwOwningPid),
                        process_name: None, // Will be filled later
                    });
                }
            }
        }

        // Get UDP table
        let mut udp_table: *mut MIB_UDPTABLE_OWNER_PID = std::ptr::null_mut();
        let mut size = 0;

        let result = GetExtendedUdpTable(
            None,
            &mut size,
            false,
            AF_INET.0 as u32,
            UDP_TABLE_OWNER_PID,
            0,
        );

        if result == 122 { // ERROR_INSUFFICIENT_BUFFER
            let mut buffer = vec![0u8; size as usize];
            udp_table = buffer.as_mut_ptr() as *mut MIB_UDPTABLE_OWNER_PID;

            let result = GetExtendedUdpTable(
                Some(udp_table as *mut _),
                &mut size,
                false,
                AF_INET.0 as u32,
                UDP_TABLE_OWNER_PID,
                0,
            );

            if result == 0 && !udp_table.is_null() {
                let table = &*udp_table;
                let num_entries = table.dwNumEntries as usize;

                for i in 0..num_entries {
                    let row_ptr = table.table.as_ptr().add(i);
                    let row = &*row_ptr;
                    let local_ip = IpAddr::V4(std::net::Ipv4Addr::from(row.dwLocalAddr.to_be()));
                    let local_port = (row.dwLocalPort as u16 >> 8) | ((row.dwLocalPort as u16 & 0xFF) << 8);

                    pids.insert(row.dwOwningPid);
                    connections.push(ConnectionRawInfo {
                        protocol: "UDP".to_string(),
                        local_addr: local_ip,
                        local_port,
                        remote_addr: IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED),
                        remote_port: 0,
                        state: "ACTIVE".to_string(),
                        pid: Some(row.dwOwningPid),
                        process_name: None, // Will be filled later
                    });
                }
            }
        }

        // Batch fetch process names for all unique PIDs
        let process_names = get_process_names_batch(&pids);

        // Fill in process names
        for conn in &mut connections {
            if let Some(pid) = conn.pid {
                conn.process_name = process_names.get(&pid).cloned();
            }
        }

        Ok(connections)
    }
}

/// Get process names for multiple PIDs using Windows native APIs (public for traffic module)
pub fn get_process_names_batch(pids: &std::collections::HashSet<u32>) -> std::collections::HashMap<u32, String> {
    use std::collections::HashMap;

    let mut result = HashMap::new();

    if pids.is_empty() {
        return result;
    }

    tracing::debug!("Getting process names for {} PIDs: {:?}", pids.len(), pids);

    unsafe {
        // First, create a snapshot of all processes
        let snapshot_result = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        let snapshot = match snapshot_result {
            Ok(h) => h,
            Err(e) => {
                tracing::error!("Failed to create process snapshot: {}", e);
                // Fall back to basic PID insertion
                for &pid in pids {
                    result.insert(pid, "-".to_string());
                }
                return result;
            }
        };

        // Create a map from PID to exe name using the snapshot
        let mut pid_to_exe: HashMap<u32, String> = HashMap::new();
        let mut entry = PROCESSENTRY32 {
            dwSize: std::mem::size_of::<PROCESSENTRY32>() as u32,
            ..Default::default()
        };

        // Iterate through all processes in the snapshot
        if Process32First(snapshot, &mut entry).is_ok() {
            loop {
                // Convert ANSI char array to string properly
                // szExeFile is CHAR array in PROCESSENTRY32, need to convert from Windows ANSI code page
                let exe_name: String = {
                    // Find null terminator
                    let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(260);

                    // Use Windows code page conversion for ANSI strings
                    // First, try to decode as Latin1 (common for Western systems)
                    // If that fails, use lossy conversion
                    let bytes: Vec<u8> = entry.szExeFile[..len].iter().map(|&c| c as u8).collect();
                    String::from_utf8_lossy(&bytes).trim_end_matches('\0').to_string()
                };
                pid_to_exe.insert(entry.th32ProcessID, exe_name);

                if Process32Next(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        tracing::debug!("Found {} processes in snapshot", pid_to_exe.len());

        let _ = CloseHandle(snapshot);

        // Now for each requested PID, try to get the full image name
        for &pid in pids {
            // Try to get full process image name (more detailed)
            if let Some(name) = try_get_process_image_name(pid) {
                tracing::debug!("PID {} -> {} (from image name)", pid, name);
                result.insert(pid, name);
            } else if let Some(exe_name) = pid_to_exe.get(&pid) {
                // Fall back to the exe name from snapshot
                tracing::debug!("PID {} -> {} (from snapshot)", pid, exe_name);
                result.insert(pid, exe_name.clone());
            } else {
                tracing::warn!("PID {} -> not found in snapshot or image lookup failed", pid);
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
        // Try with PROCESS_QUERY_LIMITED_INFORMATION first (Vista+)
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid);
        let process_handle = match handle {
            Ok(h) => h,
            Err(e) => {
                tracing::trace!("Failed to open process PID {}: {}", pid, e);
                // Try with PROCESS_QUERY_INFORMATION as fallback
                match OpenProcess(PROCESS_QUERY_INFORMATION, false, pid) {
                    Ok(h) => h,
                    Err(e) => {
                        tracing::trace!("Failed to open process PID {} with QUERY_INFO: {}", pid, e);
                        return None;
                    }
                }
            }
        };

        // Get the full image path
        let mut buffer = [0u16; 520]; // MAX_PATH * 2
        let mut size = buffer.len() as u32;

        let success = QueryFullProcessImageNameW(
            process_handle,
            windows::Win32::System::Threading::PROCESS_NAME_FORMAT(0),
            PWSTR(buffer.as_mut_ptr()),
            &mut size
        );

        let _ = CloseHandle(process_handle);

        match success {
            Ok(_) if size > 0 => {
                let full_path = OsString::from_wide(&buffer[..size as usize])
                    .to_string_lossy()
                    .into_owned();

                // Extract just the filename from the path
                if let Some(filename) = full_path.split('\\').last() {
                    if !filename.is_empty() {
                        tracing::trace!("PID {} -> {} (from QueryFullProcessImageNameW)", pid, filename);
                        return Some(filename.to_string());
                    }
                }

                tracing::trace!("PID {} -> {} (full path from QueryFullProcessImageNameW)", pid, full_path);
                Some(full_path)
            }
            Ok(_) => {
                tracing::trace!("PID {} -> QueryFullProcessImageNameW returned size 0", pid);
                None
            }
            Err(e) => {
                tracing::trace!("PID {} -> QueryFullProcessImageNameW failed: {}", pid, e);
                None
            }
        }
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

/// Get DNS servers on Windows from registry
pub fn get_dns_servers() -> anyhow::Result<Vec<String>> {
    unsafe {
        let mut hkey = windows::Win32::System::Registry::HKEY::default();

        // Open registry key for network adapters
        let result = RegOpenKeyExA(
            HKEY_LOCAL_MACHINE,
            PCSTR("SYSTEM\\CurrentControlSet\\Services\\Tcpip\\Parameters\\Interfaces\0".as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        );

        if result.is_err() {
            // Return default DNS servers
            return Ok(vec![
                "8.8.8.8".to_string(),
                "8.8.4.4".to_string(),
            ]);
        }

        // Use ipconfig as fallback to get DNS servers
        let output = std::process::Command::new("ipconfig")
            .args(&["/all"])
            .output();

        if let Ok(output) = output {
            let content = String::from_utf8_lossy(&output.stdout);
            let mut dns_servers = Vec::new();

            for line in content.lines() {
                if line.trim().starts_with("DNS Servers") {
                    if let Some(start) = line.find(':') {
                        let dns = line[start + 1..].trim();
                        if !dns.is_empty() {
                            dns_servers.push(dns.to_string());
                        }
                    }
                }
            }

            if !dns_servers.is_empty() {
                let _ = RegCloseKey(hkey);
                return Ok(dns_servers);
            }
        }

        let _ = RegCloseKey(hkey);

        Ok(vec![
            "8.8.8.8".to_string(),
            "8.8.4.4".to_string(),
        ])
    }
}

/// Set DNS servers on Windows using netsh
pub fn set_dns_servers(primary: &str, secondary: Option<&str>) -> anyhow::Result<()> {
    // Get the active interface name
    let output = std::process::Command::new("netsh")
        .args(&["interface", "show", "interface"])
        .output()?;

    let content = String::from_utf8_lossy(&output.stdout);

    // Find the connected interface
    for line in content.lines() {
        if line.contains("connected") || line.contains("专有") || line.contains("Ethernet") || line.contains("Wi-Fi") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if !parts.is_empty() {
                let interface_name = parts[0];

                // Set primary DNS
                std::process::Command::new("netsh")
                    .args(&["interface", "ip", "set", "dns", "name=", interface_name, "static", primary])
                    .output()?;

                // Set secondary DNS if provided
                if let Some(secondary) = secondary {
                    std::process::Command::new("netsh")
                        .args(&["interface", "ip", "add", "dns", "name=", interface_name, secondary, "index=2"])
                        .output()?;
                }

                return Ok(());
            }
        }
    }

    Err(anyhow::anyhow!("Failed to find active network interface"))
}

/// Kill connection on Windows by terminating the process
pub fn kill_connection(_local_addr: IpAddr, local_port: u16, _remote_addr: IpAddr, remote_port: u16) -> anyhow::Result<bool> {
    use windows::Win32::NetworkManagement::IpHelper::*;

    unsafe {
        let mut tcp_table: *mut MIB_TCPTABLE_OWNER_PID = std::ptr::null_mut();
        let mut size = 0;

        let result = GetExtendedTcpTable(
            None,
            &mut size,
            false,
            AF_INET.0 as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );

        if result == 122 {
            let mut buffer = vec![0u8; size as usize];
            tcp_table = buffer.as_mut_ptr() as *mut MIB_TCPTABLE_OWNER_PID;

            let result = GetExtendedTcpTable(
                Some(tcp_table as *mut _),
                &mut size,
                false,
                AF_INET.0 as u32,
                TCP_TABLE_OWNER_PID_ALL,
                0,
            );

            if result == 0 && !tcp_table.is_null() {
                let table = &*tcp_table;
                let num_entries = table.dwNumEntries as usize;

                for i in 0..num_entries {
                    let row_ptr = table.table.as_ptr().add(i);
                    let row = &*row_ptr;
                    let row_local_port = (row.dwLocalPort as u16 >> 8) | ((row.dwLocalPort as u16 & 0xFF) << 8);
                    let row_remote_port = (row.dwRemotePort as u16 >> 8) | ((row.dwRemotePort as u16 & 0xFF) << 8);

                    if local_port == row_local_port && remote_port == row_remote_port {
                        // Kill the process owning this connection
                        let pid = row.dwOwningPid;
                        std::process::Command::new("taskkill")
                            .args(&["/PID", &pid.to_string(), "/F"])
                            .output()?;
                        return Ok(true);
                    }
                }
            }
        }
    }

    Ok(false)
}
