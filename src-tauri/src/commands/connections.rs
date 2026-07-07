use crate::models::{ConnectionInfo, ConnectionState};
use std::net::IpAddr;

/// Get all active network connections
#[tauri::command]
pub async fn get_active_connections() -> Result<Vec<ConnectionInfo>, String> {
    // Use platform abstraction layer
    let raw_connections = crate::platform::get_active_connections().map_err(|e| e.to_string())?;

    tracing::info!(
        "Got {} raw connections from platform layer",
        raw_connections.len()
    );

    let mut connections = Vec::new();
    for raw in &raw_connections {
        let process_name = raw.process_name.as_deref().unwrap_or("-");
        tracing::debug!(
            "Connection: PID={:?}, process_name={}, remote={}",
            raw.pid,
            process_name,
            raw.remote_addr
        );

        connections.push(ConnectionInfo {
            pid: raw.pid.unwrap_or(0),
            process_name: raw.process_name.clone().unwrap_or_else(|| "-".to_string()),
            protocol: raw.protocol.clone(),
            local_address: raw.local_addr.to_string(),
            local_port: raw.local_port,
            remote_address: raw.remote_addr.to_string(),
            remote_port: raw.remote_port,
            state: parse_connection_state(&raw.state),
            remote_geoip: None,
            data_rate_bps: 0.0,
            total_bytes: 0,
        });
    }

    // Log first 5 connections for debugging
    for (i, conn) in connections.iter().take(5).enumerate() {
        tracing::info!(
            "Connection {}: process_name='{}', PID={}, remote={}",
            i,
            conn.process_name,
            conn.pid,
            conn.remote_address
        );
    }

    Ok(connections)
}

fn parse_connection_state(state: &str) -> ConnectionState {
    match state.to_uppercase().as_str() {
        "ESTABLISHED" => ConnectionState::Established,
        "LISTEN" => ConnectionState::Listening,
        "TIME_WAIT" => ConnectionState::TimeWait,
        "CLOSE_WAIT" => ConnectionState::CloseWait,
        _ => ConnectionState::Unknown,
    }
}

/// Terminate the PROCESS associated with a network connection.
///
/// **WARNING**: This kills the entire process (e.g., all browser tabs),
/// not just the specific connection. The frontend should clearly warn the user
/// before invoking this command.
#[tauri::command]
pub async fn kill_connection(
    pid: u32,
    remote_addr: String,
    remote_port: u16,
) -> Result<bool, String> {
    tracing::warn!(
        "kill_connection invoked: will terminate entire process PID={} (connection {}:{}->{}:{})",
        pid,
        "?",
        0,
        remote_addr,
        remote_port
    );
    // Validate PID range (prevent overflow and restrict reasonable range)
    const MAX_PID: u32 = 4194304; // Reasonable max PID for most systems
    if pid == 0 || pid > MAX_PID {
        return Err(format!("Invalid PID: out of valid range (1-{})", MAX_PID));
    }

    // Validate remote address
    let parsed_addr: IpAddr = remote_addr
        .parse()
        .map_err(|_| "Invalid remote address format".to_string())?;

    // Validate port is not zero
    if remote_port == 0 {
        return Err("Invalid remote port".to_string());
    }

    // Verify the connection exists before killing
    let connections = crate::platform::get_active_connections()
        .map_err(|e| format!("Failed to get connections: {}", e))?;

    let connection_exists = connections.iter().any(|conn| {
        conn.pid == Some(pid) && conn.remote_addr == parsed_addr && conn.remote_port == remote_port
    });

    if !connection_exists {
        return Err("Connection not found or PID mismatch".to_string());
    }

    // Platform-specific critical process protection
    #[cfg(windows)]
    {
        // System Idle Process (0) and System (4) are always critical
        if pid == 0 || pid == 4 {
            return Err(format!("Cannot kill system-critical process (PID {})", pid));
        }

        // Check process name to protect system processes
        if let Some(conn) = connections.iter().find(|c| c.pid == Some(pid)) {
            if let Some(ref name) = conn.process_name {
                let protected_processes = [
                    "System",
                    "smss.exe",
                    "csrss.exe",
                    "wininit.exe",
                    "winlogon.exe",
                    "services.exe",
                    "lsass.exe",
                    "svchost.exe",
                    "explorer.exe",
                    "spoolsv.exe",
                    "MsMpEng.exe",
                    "SecurityHealthService.exe",
                    "RuntimeBroker.exe",
                    "SearchIndexer.exe",
                    "dwm.exe",
                    "conhost.exe",
                    "taskhostw.exe",
                    "sihost.exe",
                    "ctfmon.exe",
                ];
                let name_lower = name.to_lowercase();
                for protected in protected_processes {
                    if name_lower == protected.to_lowercase()
                        || name_lower.ends_with(&format!("/{}", protected.to_lowercase()))
                    {
                        return Err(format!("Cannot kill protected system process: {}", name));
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Linux critical PIDs - kernel threads and essential system processes
        // Range check for kernel threads (2-500)
        if pid == 1 || (2..=500).contains(&pid) {
            return Err(format!("Cannot kill system-critical process (PID {})", pid));
        }

        // Also check process name to protect system processes
        if let Some(conn) = connections.iter().find(|c| c.pid == Some(pid)) {
            if let Some(ref name) = conn.process_name {
                let protected_processes = [
                    "systemd",
                    "init",
                    "kthreadd",
                    "ksoftirqd",
                    "migration",
                    "rcu_",
                    "chronyd",
                    "NetworkManager",
                    "sshd",
                    "dbus",
                    "polkitd",
                    "udisks2",
                    "systemd-",
                ];
                let name_lower = name.to_lowercase();
                for protected in protected_processes {
                    if name_lower == protected.to_lowercase()
                        || name_lower.starts_with(&format!("{}[", protected.to_lowercase()))
                    {
                        return Err(format!("Cannot kill protected system process: {}", name));
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS critical PIDs
        // Range check for kernel and early system processes (2-200)
        if pid == 1 || (2..=200).contains(&pid) {
            return Err(format!("Cannot kill system-critical process (PID {})", pid));
        }

        // Also check process name to protect system processes
        if let Some(conn) = connections.iter().find(|c| c.pid == Some(pid)) {
            if let Some(ref name) = conn.process_name {
                let protected_processes = [
                    "launchd",
                    "kernel_task",
                    "syslogd",
                    "configd",
                    "securityd",
                    "distnoted",
                    "mDNSResponder",
                    "powerd",
                ];
                let name_lower = name.to_lowercase();
                for protected in protected_processes {
                    if name_lower == protected.to_lowercase() {
                        return Err(format!("Cannot kill protected system process: {}", name));
                    }
                }
            }
        }
    }

    // Log the termination attempt for audit purposes
    tracing::warn!(
        "Attempting to terminate process PID={} for connection {}:{}",
        pid,
        remote_addr,
        remote_port
    );

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let output = Command::new("taskkill")
            .args(&["/PID", &pid.to_string(), "/F"])
            .output()
            .map_err(|e| format!("Failed to execute taskkill: {}", e))?;

        if output.status.success() {
            tracing::info!(
                "Killed connection: PID={}, remote={}:{}",
                pid,
                remote_addr,
                remote_port
            );
            Ok(true)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("Failed to kill process: {}", stderr))
        }
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let output = Command::new("kill")
            .args(["-15", &pid.to_string()]) // Try SIGTERM first (less aggressive)
            .output()
            .map_err(|e| format!("Failed to execute kill: {}", e))?;

        if output.status.success() {
            tracing::info!(
                "Terminated connection: PID={}, remote={}:{}",
                pid,
                remote_addr,
                remote_port
            );
            Ok(true)
        } else {
            // Fall back to SIGKILL if SIGTERM fails
            let output = Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output()
                .map_err(|e| format!("Failed to execute kill -9: {}", e))?;

            if output.status.success() {
                tracing::warn!(
                    "Force killed connection: PID={}, remote={}:{}",
                    pid,
                    remote_addr,
                    remote_port
                );
                Ok(true)
            } else {
                Err("Failed to kill process".to_string())
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        let output = Command::new("kill")
            .args(["-15", &pid.to_string()]) // Try SIGTERM first
            .output()
            .map_err(|e| format!("Failed to execute kill: {}", e))?;

        if output.status.success() {
            tracing::info!(
                "Terminated connection: PID={}, remote={}:{}",
                pid,
                remote_addr,
                remote_port
            );
            Ok(true)
        } else {
            Err("Failed to kill process".to_string())
        }
    }
}
