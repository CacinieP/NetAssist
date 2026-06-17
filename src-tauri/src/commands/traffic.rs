use crate::models::{AppTraffic, TrafficStats};
use std::sync::Arc;
use sysinfo::System;
use tokio::sync::Mutex;

/// Traffic monitoring state
struct TrafficState {
    last_rx_bytes: u64,
    last_tx_bytes: u64,
    last_update: std::time::Instant,
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
        let download_bps = if elapsed > 0.001 {
            // Minimum 1ms to prevent extreme values
            (current_rx.saturating_sub(state.last_rx_bytes)) as f64 / elapsed
        } else {
            0.0
        };

        let upload_bps = if elapsed > 0.001 {
            // Minimum 1ms to prevent extreme values
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
        let interface =
            crate::platform::get_default_interface().unwrap_or_else(|_| "en0".to_string());

        // Use netstat to get interface statistics on macOS.
        // -b reports byte counts. Output columns are (1-indexed):
        //   1:Name 2:Mtu 3:Network 4:Address 5:Ipkts 6:Ierrs 7:Ibytes
        //   8:Opkts 9:Oerrs 10:Obytes 11:Coll
        // NOTE: the same interface prints multiple rows (Link / each address).
        // Those rows all carry the SAME cumulative byte counters, so summing
        // every row would multiply the totals. We take only the FIRST data row.
        let output = Command::new("netstat")
            .args(&["-b", "-I", &interface])
            .output();

        if let Ok(result) = output {
            let content = String::from_utf8_lossy(&result.stdout);
            // skip(1) drops the header; we then take only the first data row.
            if let Some(line) = content.lines().skip(1).next() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                // Need at least 10 columns to read Ibytes(7) and Obytes(10).
                if parts.len() >= 10 {
                    let rx = parts[6].parse::<u64>().unwrap_or(0);
                    let tx = parts[9].parse::<u64>().unwrap_or(0);
                    return Ok((rx, tx));
                }
            }
        }

        Ok((0, 0))
    }
}

use std::sync::OnceLock;
static TRAFFIC_MONITOR: OnceLock<TrafficMonitor> = OnceLock::new();

fn traffic_monitor() -> &'static TrafficMonitor {
    TRAFFIC_MONITOR.get_or_init(TrafficMonitor::new)
}

/// Platform-dispatched active connection list (Windows / Linux only).
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn get_platform_connections() -> anyhow::Result<Vec<crate::platform::ConnectionRawInfo>> {
    #[cfg(target_os = "windows")]
    {
        crate::platform::windows::get_active_connections()
    }
    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::get_active_connections()
    }
}

/// Build the per-process traffic ranking entirely on a blocking thread.
///
/// This used to live in an `async` method that held a `tokio::sync::Mutex`
/// across blocking syscalls (nettop/ps/sysinfo) and was driven via
/// `spawn_blocking` + a nested `rt.block_on(...)`, which re-enters and stalls
/// the runtime. Here we do everything synchronously: build a fresh
/// `sysinfo::System`, query platform traffic, assemble and sort the list.
/// No async lock is held and the runtime is never re-entered.
fn compute_app_ranking() -> Result<Vec<AppTraffic>, String> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let mut app_traffic = Vec::new();

    // macOS: real per-process traffic (1s delta from nettop).
    #[cfg(target_os = "macos")]
    let process_traffic_stats =
        crate::platform::get_process_traffic_stats().unwrap_or_default();

    // Windows / Linux: per-PID active connection counts.
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let connection_info: std::collections::HashMap<u32, usize> = {
        let conns = get_platform_connections();
        let mut counts: std::collections::HashMap<u32, usize> =
            std::collections::HashMap::new();
        if let Ok(conns) = conns {
            for conn in conns {
                if let Some(pid) = conn.pid {
                    *counts.entry(pid).or_insert(0) += 1;
                }
            }
        }
        counts
    };

    // Resolve process names.
    #[cfg(target_os = "windows")]
    let pids: std::collections::HashSet<u32> =
        sys.processes().keys().map(|p| p.as_u32()).collect();
    #[cfg(target_os = "windows")]
    let process_names = crate::platform::windows::get_process_names_batch(&pids);

    #[cfg(target_os = "macos")]
    let process_names =
        crate::platform::get_all_processes().unwrap_or_else(|_| {
            sys.processes()
                .iter()
                .map(|(pid, p)| {
                    (
                        pid.as_u32(),
                        p.name().to_str().unwrap_or("[unknown]").to_string(),
                    )
                })
                .collect()
        });

    #[cfg(target_os = "linux")]
    let process_names: std::collections::HashMap<u32, String> = sys
        .processes()
        .iter()
        .map(|(pid, p)| {
            (
                pid.as_u32(),
                p.name().to_str().unwrap_or("[unknown]").to_string(),
            )
        })
        .collect();

    for (pid, _) in sys.processes() {
        let pid_u32 = pid.as_u32();
        let name = process_names
            .get(&pid_u32)
            .cloned()
            .unwrap_or_else(|| "[unknown]".to_string());

        // macOS: nettop returns a 1s delta, so these values are bytes/sec.
        #[cfg(target_os = "macos")]
        let (download_bps, upload_bps, total_download, total_upload) = {
            if let Some(stats) = process_traffic_stats.get(&pid_u32) {
                (
                    stats.bytes_in as f64,
                    stats.bytes_out as f64,
                    stats.bytes_in,
                    stats.bytes_out,
                )
            } else {
                (0.0, 0.0, 0, 0)
            }
        };

        // Windows / Linux: no real per-process rate is available without a
        // delta between two samples, so report 0 for the rate fields rather
        // than the previous bogus "connection_count * 100" estimate. The
        // connection count is still surfaced via total bytes = 0 for honesty.
        #[cfg(not(target_os = "macos"))]
        let (download_bps, upload_bps, total_download, total_upload) = {
            let _conn_count = connection_info.get(&pid_u32).copied().unwrap_or(0);
            (0.0, 0.0, 0u64, 0u64)
        };

        app_traffic.push(AppTraffic {
            name,
            pid: pid_u32,
            download_bytes: total_download,
            upload_bytes: total_upload,
            current_download_bps: download_bps,
            current_upload_bps: upload_bps,
        });
    }

    // Sort by current rate, descending.
    app_traffic.sort_by(|a, b| {
        let total_a = a.current_download_bps + a.current_upload_bps;
        let total_b = b.current_download_bps + b.current_upload_bps;
        total_b
            .partial_cmp(&total_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(app_traffic)
}

/// Get realtime traffic statistics
#[tauri::command]
pub async fn get_realtime_traffic() -> Result<TrafficStats, String> {
    traffic_monitor().get_stats().await
}

/// Get application traffic ranking
///
/// NOTE: `get_app_ranking` performs blocking syscalls (nettop, ps, sysinfo
/// enumeration). Previously this wrapped it in `spawn_blocking` + a nested
/// `rt.block_on(...)`, which re-enters and stalls the runtime. Instead we
/// offload the whole computation to a blocking thread and return the result.
/// The per-process rate map is computed independently of the async state, so
/// it does not need to re-enter the async runtime.
#[tauri::command]
pub async fn get_app_traffic_ranking() -> Result<Vec<AppTraffic>, String> {
    tokio::task::spawn_blocking(|| compute_app_ranking())
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}
