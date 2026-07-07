// Platform-specific implementations

// Platform-specific modules
#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

// Common utilities available on all platforms
pub mod common;

use std::net::IpAddr;

/// Network interface information (cross-platform)
#[derive(Debug, Clone)]
pub struct NetworkInterfaceInfo {
    pub name: String,
    pub display_name: String,
    pub ipv4_addresses: Vec<IpAddr>,
    pub ipv6_addresses: Vec<IpAddr>,
    pub is_up: bool,
    pub is_loopback: bool,
    pub gateway: Option<IpAddr>,
}

/// Connection information (cross-platform)
#[derive(Debug, Clone)]
pub struct ConnectionRawInfo {
    pub protocol: String,
    pub local_addr: IpAddr,
    pub local_port: u16,
    pub remote_addr: IpAddr,
    pub remote_port: u16,
    pub state: String,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
}

/// Get default gateway (cross-platform abstraction)
pub fn get_default_gateway() -> anyhow::Result<Option<IpAddr>> {
    cfg_if::cfg_if! {
        if #[cfg(windows)] {
            windows::get_default_gateway()
        } else if #[cfg(target_os = "linux")] {
            linux::get_default_gateway()
        } else if #[cfg(target_os = "macos")] {
            macos::get_default_gateway()
        } else {
            Ok(None)
        }
    }
}

/// Get default network interface (cross-platform abstraction)
pub fn get_default_interface() -> anyhow::Result<String> {
    cfg_if::cfg_if! {
        if #[cfg(windows)] {
            windows::get_default_interface()
        } else if #[cfg(target_os = "linux")] {
            linux::get_default_interface()
        } else if #[cfg(target_os = "macos")] {
            macos::get_default_interface()
        } else {
            Ok("eth0".to_string())
        }
    }
}

/// Get all network interfaces (cross-platform abstraction)
pub fn get_network_interfaces() -> anyhow::Result<Vec<NetworkInterfaceInfo>> {
    cfg_if::cfg_if! {
        if #[cfg(windows)] {
            windows::get_network_interfaces()
        } else if #[cfg(target_os = "linux")] {
            linux::get_network_interfaces()
        } else if #[cfg(target_os = "macos")] {
            macos::get_network_interfaces()
        } else {
            Ok(vec![])
        }
    }
}

/// Get active connections (cross-platform abstraction)
pub fn get_active_connections() -> anyhow::Result<Vec<ConnectionRawInfo>> {
    cfg_if::cfg_if! {
        if #[cfg(windows)] {
            windows::get_active_connections()
        } else if #[cfg(target_os = "linux")] {
            linux::get_active_connections()
        } else if #[cfg(target_os = "macos")] {
            macos::get_active_connections()
        } else {
            Ok(vec![])
        }
    }
}

/// Flush DNS cache (cross-platform abstraction)
pub fn flush_dns_cache() -> anyhow::Result<()> {
    cfg_if::cfg_if! {
        if #[cfg(windows)] {
            windows::flush_dns_cache()
        } else if #[cfg(target_os = "linux")] {
            linux::flush_dns_cache()
        } else if #[cfg(target_os = "macos")] {
            macos::flush_dns_cache()
        } else {
            Ok(())
        }
    }
}

/// Release and renew IP (cross-platform abstraction)
pub fn release_renew_ip() -> anyhow::Result<()> {
    cfg_if::cfg_if! {
        if #[cfg(windows)] {
            windows::release_renew_ip()
        } else if #[cfg(target_os = "linux")] {
            linux::release_renew_ip()
        } else if #[cfg(target_os = "macos")] {
            macos::release_renew_ip()
        } else {
            Ok(())
        }
    }
}

/// Reset network stack (cross-platform abstraction)
pub fn reset_network_stack() -> anyhow::Result<()> {
    cfg_if::cfg_if! {
        if #[cfg(windows)] {
            windows::reset_network_stack()
        } else if #[cfg(target_os = "linux")] {
            linux::reset_network_stack()
        } else if #[cfg(target_os = "macos")] {
            macos::reset_network_stack()
        } else {
            Ok(())
        }
    }
}

/// Get DNS servers (cross-platform abstraction)
pub fn get_dns_servers() -> anyhow::Result<Vec<String>> {
    cfg_if::cfg_if! {
        if #[cfg(windows)] {
            windows::get_dns_servers()
        } else if #[cfg(target_os = "linux")] {
            linux::get_dns_servers()
        } else if #[cfg(target_os = "macos")] {
            macos::get_dns_servers()
        } else {
            Ok(vec![])
        }
    }
}

/// Set DNS servers (cross-platform abstraction)
pub fn set_dns_servers(primary: &str, secondary: Option<&str>) -> anyhow::Result<()> {
    cfg_if::cfg_if! {
        if #[cfg(windows)] {
            windows::set_dns_servers(primary, secondary)
        } else if #[cfg(target_os = "linux")] {
            linux::set_dns_servers(primary, secondary)
        } else if #[cfg(target_os = "macos")] {
            macos::set_dns_servers(primary, secondary)
        } else {
            Ok(())
        }
    }
}

/// Toggle IPv6 state on the primary interface (macOS: real toggle via
/// networksetup; other platforms: not supported yet).
pub fn toggle_ipv6() -> anyhow::Result<String> {
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            macos::toggle_ipv6()
        } else {
            Err(anyhow::anyhow!("IPv6 toggle is not supported on this platform"))
        }
    }
}

/// Reset (bounce) the primary network adapter (macOS: via osascript privilege
/// escalation; other platforms: not supported yet).
pub fn reset_adapter() -> anyhow::Result<()> {
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            macos::reset_adapter()
        } else {
            Err(anyhow::anyhow!("Adapter reset is not supported on this platform"))
        }
    }
}

/// Check app permissions (platform-specific)
#[cfg(target_os = "macos")]
pub fn check_permissions() -> anyhow::Result<macos::PermissionStatus> {
    macos::check_permissions()
}

/// Check app permissions (placeholder for other platforms)
#[cfg(not(target_os = "macos"))]
pub fn check_permissions() -> anyhow::Result<PermissionStatusGeneric> {
    Ok(PermissionStatusGeneric {
        has_permissions: true,
        warnings: Vec::new(),
    })
}

/// Generic permission status for non-macOS platforms
#[derive(Debug, Clone, serde::Serialize)]
pub struct PermissionStatusGeneric {
    pub has_permissions: bool,
    pub warnings: Vec<String>,
}

/// Read cumulative (rx_bytes, tx_bytes) counters for the active network
/// interface. These are the OS-authoritative totals used by both real-time
/// rate calculation and cumulative-traffic anchoring.
///
/// Returns `Ok((0, 0))` when the counters cannot be read rather than an
/// error, so callers degrade gracefully instead of failing the whole page.
pub fn get_interface_total_bytes() -> (u64, u64) {
    cfg_if::cfg_if! {
        if #[cfg(windows)] {
            windows::get_interface_total_bytes()
        } else if #[cfg(target_os = "linux")] {
            linux::get_interface_total_bytes()
        } else if #[cfg(target_os = "macos")] {
            macos::get_interface_total_bytes()
        } else {
            (0, 0)
        }
    }
}

/// Get process traffic stats (platform-specific)
#[cfg(target_os = "macos")]
pub fn get_process_traffic_stats(
) -> anyhow::Result<std::collections::HashMap<u32, macos::ProcessTrafficStats>> {
    macos::get_process_traffic_stats()
}

/// Get all processes (platform-specific)
#[cfg(target_os = "macos")]
pub fn get_all_processes() -> anyhow::Result<std::collections::HashMap<u32, String>> {
    macos::get_all_processes()
}

/// Detect network interface changes (macOS)
#[cfg(target_os = "macos")]
pub fn detect_interface_changes() -> anyhow::Result<macos::InterfaceChangeEvent> {
    macos::detect_interface_changes()
}

/// Run macOS-specific network diagnostics
#[cfg(target_os = "macos")]
pub fn run_macos_diagnostics() -> anyhow::Result<macos::MacOSDiagnostics> {
    macos::run_network_diagnostics()
}
