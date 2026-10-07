use crate::models::{AppTraffic, TrafficStats};
use std::sync::{Arc, Mutex};
use sysinfo::System;

/// Minimum spacing between OS-counter samples when running on battery.
///
/// The frontend polls every second regardless; when unplugged, polls inside
/// this window are answered from `TrafficState::last_stats` without spawning
/// any subprocess, cutting counter sampling (and wakeups) 5×. Plugged in,
/// every poll samples as before.
const BATTERY_MIN_SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// Traffic monitoring state.
///
/// The baseline (last observed interface counters + the time they were read)
/// is optional: before the FIRST successful counter read we have no baseline,
/// so the first poll reports 0 bps instead of "uptime bytes / uptime seconds"
/// (which produced GB/s spikes and poisoned history).
struct TrafficState {
    source: Option<String>,
    last_rx_bytes: Option<u64>,
    last_tx_bytes: Option<u64>,
    last_update: Option<std::time::Instant>,
    /// Last computed stats, kept so battery-throttled polls can be answered
    /// without touching the OS counters.
    last_stats: Option<TrafficStats>,
}

pub struct TrafficMonitor {
    state: Arc<Mutex<TrafficState>>,
}

impl TrafficMonitor {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(TrafficState {
                source: None,
                last_rx_bytes: None,
                last_tx_bytes: None,
                last_update: None,
                last_stats: None,
            })),
        }
    }

    /// Serialize physical reads with their baseline update. A failed sample
    /// never replaces a valid baseline with fabricated zero counters.
    pub async fn get_stats(&self) -> Result<TrafficStats, String> {
        let state = Arc::clone(&self.state);
        tokio::task::spawn_blocking(move || {
            // This mutex is used only on blocking threads; holding it over the
            // OS read prevents an older sample from committing after a newer one.
            let mut state = state.lock().map_err(|e| format!("Lock error: {}", e))?;
            if crate::core::power::is_on_battery_cached()
                && state
                    .last_update
                    .is_some_and(|t| t.elapsed() < BATTERY_MIN_SAMPLE_INTERVAL)
            {
                if let Some(stats) = state.last_stats.clone() {
                    return Ok(stats);
                }
            }
            let snapshot = crate::platform::get_interface_total_bytes()?;
            Ok(state.update(snapshot, std::time::Instant::now()))
        })
        .await
        .map_err(|e| format!("Task join error: {}", e))?
    }
}

impl TrafficState {
    fn update(
        &mut self,
        sample: crate::platform::CounterSnapshot,
        now: std::time::Instant,
    ) -> TrafficStats {
        let mut download_bps = 0.0;
        let mut upload_bps = 0.0;
        if self.source.as_deref() == Some(sample.source.as_str()) {
            if let (Some(last_rx), Some(last_tx), Some(last_update)) =
                (self.last_rx_bytes, self.last_tx_bytes, self.last_update)
            {
                let elapsed = now.saturating_duration_since(last_update).as_secs_f64();
                if elapsed > 0.001 && sample.rx >= last_rx && sample.tx >= last_tx {
                    download_bps = (sample.rx - last_rx) as f64 / elapsed;
                    upload_bps = (sample.tx - last_tx) as f64 / elapsed;
                }
            }
        }
        self.source = Some(sample.source);
        self.last_rx_bytes = Some(sample.rx);
        self.last_tx_bytes = Some(sample.tx);
        self.last_update = Some(now);
        let stats = TrafficStats {
            download_bps,
            upload_bps,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };
        self.last_stats = Some(stats.clone());
        stats
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
        crate::platform::get_process_traffic_stats().map_err(|error| error.to_string())?;

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

        // macOS: nettop returns interval bytes; divide by its measured duration.
        #[cfg(target_os = "macos")]
        let (
            download_bps,
            upload_bps,
            total_download,
            total_upload,
            traffic_available,
            sample_seconds,
        ) = {
            if let Some(stats) = process_traffic_stats.get(&pid_u32) {
                (
                    stats.bytes_in as f64 / stats.sample_seconds,
                    stats.bytes_out as f64 / stats.sample_seconds,
                    stats.bytes_in,
                    stats.bytes_out,
                    true,
                    Some(stats.sample_seconds),
                )
            } else {
                (0.0, 0.0, 0, 0, false, None)
            }
        };

        // Windows / Linux: no real per-process rate is available without a
        // delta between two samples, so report 0 for the rate fields rather
        // than the previous bogus "connection_count * 100" estimate. The
        // connection count is still surfaced via total bytes = 0 for honesty.
        #[cfg(not(target_os = "macos"))]
        let (
            download_bps,
            upload_bps,
            total_download,
            total_upload,
            traffic_available,
            sample_seconds,
        ) = {
            let _conn_count = connection_info.get(&pid_u32).copied().unwrap_or(0);
            (0.0, 0.0, 0u64, 0u64, false, None)
        };

        app_traffic.push(AppTraffic {
            name,
            pid: pid_u32,
            traffic_available,
            sample_seconds,
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

/// Get OS interface total bytes (rx/tx counters).
///
/// Exposed as a Tauri command so the frontend can include OS-level
/// cumulative traffic in exports as a fallback when per-process
/// data (nettop) is unavailable.
#[tauri::command]
pub async fn get_interface_counters() -> Result<(u64, u64), String> {
    tokio::task::spawn_blocking(|| {
        let snapshot = crate::platform::get_interface_total_bytes()?;
        Ok((snapshot.rx, snapshot.tx))
    })
    .await
    .map_err(|error| format!("Task join error: {}", error))?
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

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> TrafficState {
        TrafficState {
            source: None,
            last_rx_bytes: None,
            last_tx_bytes: None,
            last_update: None,
            last_stats: None,
        }
    }
    fn snapshot(source: &str, rx: u64, tx: u64) -> crate::platform::CounterSnapshot {
        crate::platform::CounterSnapshot {
            source: source.into(),
            rx,
            tx,
        }
    }
    #[test]
    fn realtime_rate_uses_same_source_delta_and_rebases_reset_or_switch() {
        let mut state = state();
        let start = std::time::Instant::now();
        assert_eq!(
            state
                .update(snapshot("en0", 1_000_000, 500), start)
                .download_bps,
            0.0
        );
        let next = state.update(
            snapshot("en0", 1_001_000, 700),
            start + std::time::Duration::from_secs(2),
        );
        assert_eq!(next.download_bps, 500.0);
        assert_eq!(next.upload_bps, 100.0);
        let switched = state.update(
            snapshot("utun0", 90_000_000, 80_000_000),
            start + std::time::Duration::from_secs(3),
        );
        assert_eq!(switched.download_bps, 0.0);
        let reset = state.update(
            snapshot("utun0", 100, 50),
            start + std::time::Duration::from_secs(4),
        );
        assert_eq!(reset.download_bps, 0.0);
        let recovered = state.update(
            snapshot("utun0", 1100, 250),
            start + std::time::Duration::from_secs(6),
        );
        assert_eq!(recovered.download_bps, 500.0);
    }
}
