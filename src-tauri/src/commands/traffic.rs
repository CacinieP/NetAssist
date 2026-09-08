use crate::models::{AppTraffic, TrafficStats};
use std::sync::Arc;
use sysinfo::System;
use tokio::sync::Mutex;

/// Traffic monitoring state.
///
/// The baseline (last observed interface counters + the time they were read)
/// is optional: before the FIRST successful counter read we have no baseline,
/// so the first poll reports 0 bps instead of "uptime bytes / uptime seconds"
/// (which produced GB/s spikes and poisoned history).
struct TrafficState {
    last_rx_bytes: Option<u64>,
    last_tx_bytes: Option<u64>,
    last_update: Option<std::time::Instant>,
}

pub struct TrafficMonitor {
    state: Arc<Mutex<TrafficState>>,
}

impl TrafficMonitor {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(TrafficState {
                last_rx_bytes: None,
                last_tx_bytes: None,
                last_update: None,
            })),
        }
    }

    /// Get current traffic statistics
    pub async fn get_stats(&self) -> Result<TrafficStats, String> {
        // Read the OS counters OUTSIDE the lock (get_interface_total_bytes()
        // may spawn external commands on macOS; holding the async mutex while
        // doing blocking work would stall other commands).
        let (current_rx, current_tx) =
            tokio::task::spawn_blocking(crate::platform::get_interface_total_bytes)
                .await
                .unwrap_or((0, 0));

        let now = std::time::Instant::now();
        let mut state = self.state.lock().await;

        // Compute rate from the previous reading; no previous reading yet → 0.
        let mut download_bps = 0.0f64;
        let mut upload_bps = 0.0f64;
        if let (Some(last_rx), Some(last_tx), Some(last_update)) =
            (state.last_rx_bytes, state.last_tx_bytes, state.last_update)
        {
            let elapsed = now.duration_since(last_update).as_secs_f64();
            if elapsed > 0.001 {
                download_bps = (current_rx.saturating_sub(last_rx)) as f64 / elapsed;
                upload_bps = (current_tx.saturating_sub(last_tx)) as f64 / elapsed;
            }
        }

        // Update state
        state.last_rx_bytes = Some(current_rx);
        state.last_tx_bytes = Some(current_tx);
        state.last_update = Some(now);

        Ok(TrafficStats {
            download_bps,
            upload_bps,
            timestamp: chrono::Utc::now().timestamp_millis(),
        })
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
    let process_traffic_stats = crate::platform::get_process_traffic_stats().unwrap_or_default();

    // Windows / Linux: per-PID active connection counts.
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let connection_info: std::collections::HashMap<u32, usize> = {
        let conns = get_platform_connections();
        let mut counts: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
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
    let pids: std::collections::HashSet<u32> = sys.processes().keys().map(|p| p.as_u32()).collect();
    #[cfg(target_os = "windows")]
    let process_names = crate::platform::windows::get_process_names_batch(&pids);

    #[cfg(target_os = "macos")]
    let process_names = crate::platform::get_all_processes().unwrap_or_else(|_| {
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

    for pid in sys.processes().keys() {
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
    tokio::task::spawn_blocking(compute_app_ranking)
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}
