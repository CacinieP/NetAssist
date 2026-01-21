#![allow(dead_code)]

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
