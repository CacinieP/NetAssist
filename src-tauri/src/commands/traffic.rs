use crate::models::{TrafficStats, AppTraffic};
use std::sync::Arc;
use tokio::sync::Mutex;
use sysinfo::System;

/// Traffic monitoring state
struct TrafficState {
    last_rx_bytes: u64,
    last_tx_bytes: u64,
    last_update: std::time::Instant,
    app_traffic: std::collections::HashMap<u32, AppTrafficData>,
}

struct AppTrafficData {
    name: String,
    download_bytes: u64,
    upload_bytes: u64,
    last_download: f64,
    last_upload: f64,
}

pub struct TrafficMonitor {
    state: Arc<Mutex<TrafficState>>,
}

impl TrafficMonitor {
    pub fn new() -> Self {
        let (rx_bytes, tx_bytes) = (0, 0);

        Self {
            state: Arc::new(Mutex::new(TrafficState {
                last_rx_bytes: rx_bytes,
                last_tx_bytes: tx_bytes,
                last_update: std::time::Instant::now(),
                app_traffic: std::collections::HashMap::new(),
            })),
        }
    }

    /// Get current traffic statistics
    pub async fn get_stats(&self) -> Result<TrafficStats, String> {
        let mut state = self.state.lock().await;
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(state.last_update).as_secs_f64();

        let (current_rx, current_tx) = self.get_interface_stats()?;

        // Calculate rates with minimum elapsed time check
        let download_bps = if elapsed > 0.001 {  // Minimum 1ms to prevent extreme values
            (current_rx.saturating_sub(state.last_rx_bytes)) as f64 / elapsed
        } else {
            0.0
        };

        let upload_bps = if elapsed > 0.001 {  // Minimum 1ms to prevent extreme values
            (current_tx.saturating_sub(state.last_tx_bytes)) as f64 / elapsed
        } else {
            0.0
        };

        // Update state
        state.last_rx_bytes = current_rx;
        state.last_tx_bytes = current_tx;
        state.last_update = now;

        Ok(TrafficStats {
            download_bps,
            upload_bps,
            timestamp: chrono::Utc::now().timestamp_millis(),
        })
    }

    /// Get application traffic ranking
    pub async fn get_app_ranking(&self) -> Result<Vec<AppTraffic>, String> {
        let mut sys = System::new_all();
        sys.refresh_all();

        let mut app_traffic = Vec::new();

        // macOS: Use nettop for real process traffic data
        #[cfg(target_os = "macos")]
        let process_traffic_stats = {
            match crate::platform::get_process_traffic_stats() {
                Ok(stats) => stats,
                Err(_) => std::collections::HashMap::new(),
            }
        };

        // Windows: Use connection info for traffic estimation
        #[cfg(target_os = "windows")]
        let connection_info = {
            match crate::platform::windows::get_active_connections() {
                Ok(conns) => {
                    // Count connections per PID
                    let mut conn_count: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
                    for conn in conns {
                        if let Some(pid) = conn.pid {
                            *conn_count.entry(pid).or_insert(0) += 1;
                        }
                    }
                    conn_count
                }
                Err(_) => std::collections::HashMap::new(),
            }
        };

        // Linux: Use ss for process info
        #[cfg(target_os = "linux")]
        let connection_info = {
            match crate::platform::linux::get_active_connections() {
                Ok(conns) => {
                    let mut conn_count: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
                    for conn in conns {
                        if let Some(pid) = conn.pid {
                            *conn_count.entry(pid).or_insert(0) += 1;
                        }
                    }
                    conn_count
                }
                Err(_) => std::collections::HashMap::new(),
            }
        };

        // Collect all PIDs first
        let mut pids = std::collections::HashSet::new();
        for (pid, _) in sys.processes() {
            pids.insert(pid.as_u32());
        }

        // Use our improved process name resolution
        #[cfg(target_os = "windows")]
        let process_names = crate::platform::windows::get_process_names_batch(&pids);

        #[cfg(target_os = "macos")]
        let process_names = {
            match crate::platform::get_all_processes() {
                Ok(map) => map,
                Err(_) => {
                    let mut map = std::collections::HashMap::new();
                    for (pid, process) in sys.processes() {
                        let name = process.name().to_str().unwrap_or("[unknown]").to_string();
                        map.insert(pid.as_u32(), name);
                    }
                    map
                }
            }
        };

        #[cfg(target_os = "linux")]
        let process_names = {
            let mut map = std::collections::HashMap::new();
            for (pid, process) in sys.processes() {
                let name = process.name().to_str().unwrap_or("[unknown]").to_string();
                map.insert(pid.as_u32(), name);
            }
            map
        };

        // Build app traffic list
        for (pid, _) in sys.processes() {
            let pid_u32 = pid.as_u32();
            let name = process_names
                .get(&pid_u32)
                .cloned()
                .unwrap_or_else(|| "[unknown]".to_string());

            // On macOS, use real traffic data from nettop
            #[cfg(target_os = "macos")]
            let (download_bps, upload_bps) = {
                if let Some(stats) = process_traffic_stats.get(&pid_u32) {
                    // Convert bytes to bits per second (rough estimate over the sampling interval)
                    // nettop gives cumulative bytes, so we estimate bps
                    let total_bps = (stats.bytes_in + stats.bytes_out) as f64 * 8.0 / 60.0; // Assume 60s interval
                    (total_bps * 0.7, total_bps * 0.3) // 70% download, 30% upload
                } else {
                    (0.0, 0.0)
                }
            };

            // On Windows/Linux, use connection count as proxy
            #[cfg(not(target_os = "macos"))]
            let (download_bps, upload_bps) = {
                let conn_count = connection_info.get(&pid_u32).copied().unwrap_or(0);
                let estimated_bps = conn_count as f64 * 100.0;
                (estimated_bps, estimated_bps * 0.3)
            };

            // Get cumulative bytes if available
            #[cfg(target_os = "macos")]
            let (total_download, total_upload) = {
                if let Some(stats) = process_traffic_stats.get(&pid_u32) {
                    (stats.bytes_in, stats.bytes_out)
                } else {
                    (0, 0)
                }
            };

            #[cfg(not(target_os = "macos"))]
            let (total_download, total_upload) = (0, 0);

            app_traffic.push(AppTraffic {
                name,
                pid: pid_u32,
                download_bytes: total_download,
                upload_bytes: total_upload,
                current_download_bps: download_bps,
                current_upload_bps: upload_bps,
            });
        }

        // Sort by estimated traffic (descending)
        app_traffic.sort_by(|a, b| {
            let total_a = a.current_download_bps + a.current_upload_bps;
            let total_b = b.current_download_bps + b.current_upload_bps;
            total_b.partial_cmp(&total_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(app_traffic)
    }

    /// Get interface statistics (platform-specific)
    #[cfg(target_os = "windows")]
    fn get_interface_stats(&self) -> Result<(u64, u64), String> {
        use windows::Win32::NetworkManagement::IpHelper::*;

        unsafe {
            // GetIfTable2 allocates memory and returns a pointer
            let mut if_table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();

            // GetIfTable2 returns WIN32_ERROR
            let result = GetIfTable2(&mut if_table);

            if result.is_ok() && !if_table.is_null() {
                let table = &*if_table;
                let mut total_rx = 0u64;
                let mut total_tx = 0u64;

                // Sum statistics from all interfaces
                // Safety: Only iterate through actual number of entries
                let num_entries = table.NumEntries as usize;
                let table_ptr = table.Table.as_ptr();

                for i in 0..num_entries {
                    let row = &*table_ptr.add(i);
                    // Check if interface is operational (OperStatus == 1 = Up)
                    if row.OperStatus.0 == 1 {
                        total_rx += row.InOctets;
                        total_tx += row.OutOctets;
                    }
                }

                // Free the memory allocated by GetIfTable2
                FreeMibTable(if_table as *mut _);

                Ok((total_rx, total_tx))
            } else {
                // Fallback: return zeros if we can't get stats
                Ok((0, 0))
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn get_interface_stats(&self) -> Result<(u64, u64), String> {
        use std::fs;

        let mut total_rx = 0u64;
        let mut total_tx = 0u64;

        // Read from /proc/net/dev
        if let Ok(content) = fs::read_to_string("/proc/net/dev") {
            for line in content.lines().skip(2) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 10 {
                    if let Ok(rx) = parts[1].parse::<u64>() {
                        total_rx += rx;
                    }
                    if let Ok(tx) = parts[9].parse::<u64>() {
                        total_tx += tx;
                    }
                }
            }
        }

        Ok((total_rx, total_tx))
    }

    #[cfg(target_os = "macos")]
    fn get_interface_stats(&self) -> Result<(u64, u64), String> {
        use std::process::Command;

        // Get the active interface dynamically
        let interface = crate::platform::get_default_interface()
            .unwrap_or_else(|_| "en0".to_string());

        // Use netstat to get interface statistics on macOS
        let output = Command::new("netstat")
            .args(&["-b", "-I", &interface])
            .output();

        let mut total_rx = 0u64;
        let mut total_tx = 0u64;

        if let Ok(result) = output {
            let content = String::from_utf8_lossy(&result.stdout);
            // Parse netstat output: "Ibytes" and "Obytes" columns
            for line in content.lines().skip(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 10 {
                    // netstat -b output format
                    if let Ok(rx) = parts[7].parse::<u64>() {
                        total_rx += rx;
                    }
                    if parts.len() > 10 {
                        if let Ok(tx) = parts[10].parse::<u64>() {
                            total_tx += tx;
                        }
                    }
                }
            }
        }

        Ok((total_rx, total_tx))
    }
}

lazy_static::lazy_static! {
    static ref TRAFFIC_MONITOR: TrafficMonitor = TrafficMonitor::new();
}

/// Get realtime traffic statistics
#[tauri::command]
pub async fn get_realtime_traffic() -> Result<TrafficStats, String> {
    TRAFFIC_MONITOR.get_stats().await
}

/// Get application traffic ranking
#[tauri::command]
pub async fn get_app_traffic_ranking() -> Result<Vec<AppTraffic>, String> {
    TRAFFIC_MONITOR.get_app_ranking().await
}
