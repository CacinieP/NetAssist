//! Traffic history and alert management commands

use crate::models::{
    AlertStatus, CumulativeTraffic, TrafficAlert, TrafficHistory, TrafficHistoryPoint,
};
use chrono::{DateTime, Datelike, Local, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Convert epoch seconds to a local DateTime (epoch → UTC → local).
fn local_from_epoch(secs: i64) -> Option<DateTime<Local>> {
    Utc.timestamp_opt(secs, 0)
        .single()
        .map(|u| u.with_timezone(&Local))
}

/// Traffic history storage state
struct TrafficHistoryStorage {
    data_dir: PathBuf,
    history_cache: HashMap<String, Vec<TrafficHistoryPoint>>,
}

impl TrafficHistoryStorage {
    fn new() -> Result<Self, String> {
        let config_dir =
            dirs::config_dir().ok_or_else(|| "Could not find config directory".to_string())?;

        let data_dir = config_dir.join("NetAssist").join("traffic");
        fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        Ok(Self {
            data_dir,
            history_cache: HashMap::new(),
        })
    }

    /// Validate date string format (YYYY-MM-DD) to prevent path traversal
    fn validate_date_format(date_str: &str) -> Result<(), String> {
        // Check length (YYYY-MM-DD = 10 chars)
        if date_str.len() != 10 {
            return Err("Invalid date format: must be YYYY-MM-DD".to_string());
        }

        // Check for NULL bytes and other dangerous characters
        if date_str.contains('\0') || date_str.contains('\n') || date_str.contains('\r') {
            return Err("Invalid date format: contains control characters".to_string());
        }

        // Check for path traversal attempts
        if date_str.contains("..") || date_str.contains('/') || date_str.contains('\\') {
            return Err("Invalid date format: contains path separators".to_string());
        }

        // Validate format using chrono
        use chrono::NaiveDate;
        if NaiveDate::parse_from_str(date_str, "%Y-%m-%d").is_err() {
            return Err("Invalid date format: must be valid YYYY-MM-DD date".to_string());
        }

        // Reasonable date range check (years 2000-2100)
        let year = date_str[0..4]
            .parse::<i32>()
            .map_err(|_| "Invalid year".to_string())?;
        if !(2000..=2100).contains(&year) {
            return Err("Date out of valid range (2000-2100)".to_string());
        }

        Ok(())
    }

    /// Get file path for a specific date (with validation)
    fn get_date_file_path(&self, date_str: &str) -> Result<PathBuf, String> {
        // Validate date string before using it in file path
        Self::validate_date_format(date_str)?;

        Ok(self.data_dir.join(format!("{}.json", date_str)))
    }

    /// Load traffic history for a specific date
    fn load_day_history(&self, date_str: &str) -> Result<Vec<TrafficHistoryPoint>, String> {
        let file_path = self.get_date_file_path(date_str)?;

        if file_path.exists() {
            let content = fs::read_to_string(&file_path)
                .map_err(|e| format!("Failed to read history file: {}", e))?;
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse history file: {}", e))
        } else {
            Ok(Vec::new())
        }
    }

    /// Save traffic history for a specific date (atomic: write temp + rename)
    fn save_day_history(&self, date_str: &str, data: &[TrafficHistoryPoint]) -> Result<(), String> {
        let file_path = self.get_date_file_path(date_str)?;
        let content = serde_json::to_string_pretty(data)
            .map_err(|e| format!("Failed to serialize history: {}", e))?;

        let tmp_path = file_path.with_extension("json.tmp");
        fs::write(&tmp_path, content)
            .map_err(|e| format!("Failed to write history file: {}", e))?;
        fs::rename(&tmp_path, &file_path)
            .map_err(|e| format!("Failed to finalize history file: {}", e))?;
        Ok(())
    }

    /// Add a new traffic data point (stored under the LOCAL calendar date).
    fn add_data_point(&mut self, download_bps: f64, upload_bps: f64) -> Result<(), String> {
        let now = Local::now();
        let date_str = now.format("%Y-%m-%d").to_string();
        let timestamp = now.timestamp_millis();

        let point = TrafficHistoryPoint {
            timestamp,
            download_bps,
            upload_bps,
        };

        // Load existing data for the day
        let mut day_data = self.load_day_history(&date_str)?;
        day_data.push(point);

        // Keep only the last 24h of points (window-based, not count-based).
        let cutoff_ms = now.timestamp_millis() - 24 * 60 * 60 * 1000;
        day_data.retain(|p| p.timestamp >= cutoff_ms);
        day_data.sort_by_key(|p| p.timestamp);

        self.save_day_history(&date_str, &day_data)?;
        self.history_cache.insert(date_str, day_data);

        Ok(())
    }

    /// Get cumulative traffic for a time period (per-point file aggregation).
    ///
    /// Each sample represents a byte rate that applies until the NEXT sample
    /// (or, for the last one, until "now" / the period end). Gaps larger than
    /// 5 minutes are capped so an app-shutdown gap can't multiply the stale
    /// rate across the whole downtime.
    fn get_cumulative_traffic(&self, period: &str) -> Result<CumulativeTraffic, String> {
        let now = Local::now();
        let (start_time, end_time, period_label) = Self::period_bounds(period, now)?;

        let mut total_download = 0u64;
        let mut total_upload = 0u64;

        // Load data for each day in the period (local calendar days)
        let start_date = local_from_epoch(start_time)
            .unwrap_or_else(Local::now);
        let end_date = local_from_epoch(end_time).unwrap_or_else(Local::now);
        let mut current_date = start_date;

        // Convert to milliseconds for comparison with point.timestamp
        let start_time_ms = start_time * 1000;
        let end_time_ms = end_time * 1000;

        while current_date <= end_date {
            let date_str = current_date.format("%Y-%m-%d").to_string();
            if let Ok(day_data) = self.load_day_history(&date_str) {
                // Sort by timestamp to ensure correct order
                let mut sorted_data = day_data.clone();
                sorted_data.sort_by_key(|p| p.timestamp);

                for (i, point) in sorted_data.iter().enumerate() {
                    if point.timestamp < start_time_ms || point.timestamp > end_time_ms {
                        continue;
                    }
                    // The rate holds until the next sample; the last sample
                    // holds until the period end (== now). Cap at 5 min so a
                    // large sampling gap can't inflate the total.
                    let end_eff = sorted_data
                        .get(i + 1)
                        .map(|n| n.timestamp)
                        .unwrap_or(end_time_ms)
                        .min(end_time_ms);
                    if end_eff <= point.timestamp {
                        continue;
                    }
                    let interval_seconds = ((end_eff - point.timestamp) as f64 / 1000.0)
                        .min(300.0);
                    if interval_seconds <= 0.0 {
                        continue;
                    }

                    total_download += (point.download_bps * interval_seconds) as u64;
                    total_upload += (point.upload_bps * interval_seconds) as u64;
                }
            }
            current_date += chrono::Duration::days(1);
        }

        Ok(CumulativeTraffic {
            total_download_bytes: total_download,
            total_upload_bytes: total_upload,
            start_timestamp: start_time,
            end_timestamp: end_time,
            period: period_label,
        })
    }

    /// Get traffic history for a time range (hours, 1..=24*14 capped)
    fn get_traffic_history(&self, hours: i64) -> Result<TrafficHistory, String> {
        // Guard: an absurd hours value would build an enormous loop / overflow
        // chrono::Duration. Cap the useful window (14 days of points).
        let hours = hours.clamp(1, 24 * 14);
        let now = Local::now();
        let start_time = now - chrono::Duration::hours(hours);

        let mut all_data = Vec::new();

        let start_date =
            local_from_epoch(start_time.timestamp()).unwrap_or_else(Local::now);
        let mut current_date = start_date;

        while current_date <= now {
            let date_str = current_date.format("%Y-%m-%d").to_string();
            if let Ok(day_data) = self.load_day_history(&date_str) {
                for point in day_data {
                    if point.timestamp >= start_time.timestamp_millis()
                        && point.timestamp <= now.timestamp_millis()
                    {
                        all_data.push(point);
                    }
                }
            }
            current_date += chrono::Duration::days(1);
        }

        all_data.sort_by_key(|p| p.timestamp);

        Ok(TrafficHistory {
            data: all_data,
            start_timestamp: start_time.timestamp_millis(),
            end_timestamp: now.timestamp_millis(),
        })
    }

    /// Compute the (start, end, label) bounds for a period relative to `now`
    /// in the LOCAL timezone. History files and trend charts label the day by
    /// local date, so boundaries must match the local calendar — with UTC,
    /// "today" for UTC+8 started at 08:00 local and the week started 8h late.
    fn period_bounds(period: &str, now: DateTime<Local>) -> Result<(i64, i64, String), String> {
        use chrono::TimeZone;
        // Local midnight for a given date.
        let midnight = |d: chrono::NaiveDate| -> Option<DateTime<Local>> {
            d.and_hms_opt(0, 0, 0)
                .and_then(|ndt| Local.from_local_datetime(&ndt).single())
        };

        match period {
            "day" => {
                let start = midnight(now.date_naive())
                    .map(|d| d.timestamp())
                    .unwrap_or(now.timestamp());
                Ok((start, now.timestamp(), "day".to_string()))
            }
            "week" => {
                let weekday = now.weekday().num_days_from_monday();
                let today_midnight = midnight(now.date_naive()).unwrap_or(now);
                let start = today_midnight - chrono::Duration::days(weekday as i64);
                Ok((start.timestamp(), now.timestamp(), "week".to_string()))
            }
            "month" => {
                let first = now
                    .date_naive()
                    .with_day(1)
                    .ok_or_else(|| "Invalid month".to_string())?;
                let start = midnight(first)
                    .map(|d| d.timestamp())
                    .unwrap_or(now.timestamp());
                Ok((start, now.timestamp(), "month".to_string()))
            }
            _ => Err(format!("Invalid period: {}", period)),
        }
    }

    /// Path to the on-disk anchor store (interface-counter snapshots taken at
    /// the start of each period). Lives next to the per-day history files.
    fn anchors_file_path(&self) -> PathBuf {
        self.data_dir.join("anchors.json")
    }

    /// Load the anchor store. Returns an empty store if the file is missing or
    /// corrupt (we never want a bad anchor file to blank the whole page).
    fn load_anchors(&self) -> AnchorStore {
        let path = self.anchors_file_path();
        if !path.exists() {
            return AnchorStore::default();
        }
        match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => AnchorStore::default(),
        }
    }

    /// Persist the anchor store. Failures are logged but non-fatal.
    fn save_anchors(&self, anchors: &AnchorStore) {
        let path = self.anchors_file_path();
        if let Ok(content) = serde_json::to_string_pretty(anchors) {
            let tmp = path.with_extension("tmp");
            if fs::write(&tmp, &content).is_ok() {
                let _ = fs::rename(tmp, path);
            } else if let Err(e) = fs::write(&path, content) {
                tracing::warn!("Failed to write traffic anchors file: {}", e);
            }
        }
    }

    /// Cumulative traffic driven by the OS interface byte counters.
    fn get_cumulative_traffic_via_counters(
        &mut self,
        period: &str,
    ) -> Result<CumulativeTraffic, String> {
        let (current_rx, current_tx) = crate::platform::get_interface_total_bytes();
        self.cumulative_from_counters(period, current_rx, current_tx)
    }

    /// Pure accumulator update (explicit counters → testable).
    ///
    /// Robust "accrued counter" model: each period stores the last observed
    /// counter and the traffic already accrued. Every poll adds
    /// `current − last` when the counter only grew, and simply re-anchors
    /// `last = current` when the counter went backwards (interface flap,
    /// reboot, Wi-Fi switch, docker/veth removal, app restart). The accrued
    /// total is therefore NEVER zeroed by a counter reset — only a real
    /// period rollover (new day/week/month) starts the accumulator at 0.
    fn cumulative_from_counters(
        &mut self,
        period: &str,
        current_rx: u64,
        current_tx: u64,
    ) -> Result<CumulativeTraffic, String> {
        let now = Local::now();
        let (start_ts, end_ts, label) = Self::period_bounds(period, now)?;

        let mut anchors = self.load_anchors();

        {
            // Resolve the anchor for this period.
            let anchor = match period {
                "week" => &mut anchors.week,
                "month" => &mut anchors.month,
                // default to day for "day" and any unrecognized value
                _ => &mut anchors.day,
            };

            // New period (day/week/month rollover): start the accumulator at 0.
            if anchor.start_ts != start_ts {
                anchor.start_ts = start_ts;
                anchor.last_rx = current_rx;
                anchor.last_tx = current_tx;
                anchor.accrued_rx = 0;
                anchor.accrued_tx = 0;
            } else {
                // Same period: accrue only forward counter movement.
                if current_rx >= anchor.last_rx {
                    anchor.accrued_rx = anchor
                        .accrued_rx
                        .saturating_add(current_rx - anchor.last_rx);
                }
                if current_tx >= anchor.last_tx {
                    anchor.accrued_tx = anchor
                        .accrued_tx
                        .saturating_add(current_tx - anchor.last_tx);
                }
                // If counters went backwards (reset/flap), we do NOT subtract —
                // just re-anchor so future growth accrues from here.
                anchor.last_rx = current_rx;
                anchor.last_tx = current_tx;
            }
        }

        // Persist whenever anything changed (or first run).
        self.save_anchors(&anchors);

        Ok(CumulativeTraffic {
            total_download_bytes: match period {
                "week" => anchors.week.accrued_rx,
                "month" => anchors.month.accrued_rx,
                _ => anchors.day.accrued_rx,
            },
            total_upload_bytes: match period {
                "week" => anchors.week.accrued_tx,
                "month" => anchors.month.accrued_tx,
                _ => anchors.day.accrued_tx,
            },
            start_timestamp: start_ts,
            end_timestamp: end_ts,
            period: label,
        })
    }
}

/// Per-period accumulator state.
///
/// - `start_ts` marks which calendar period the accumulator belongs to
///   (rollover detection).
/// - `last_rx/last_tx` are the previously observed OS counters.
/// - `accrued_rx/accrued_tx` is the cumulative traffic for the period so far,
///   which is only ever added to (never reset by counter flapping).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
struct PeriodAnchor {
    start_ts: i64,
    last_rx: u64,
    last_tx: u64,
    accrued_rx: u64,
    accrued_tx: u64,
}

/// On-disk anchor store: one anchor per supported period. Persisted at
/// `traffic/anchors.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AnchorStore {
    day: PeriodAnchor,
    week: PeriodAnchor,
    month: PeriodAnchor,
}

/// Traffic alert manager
struct TrafficAlertManager {
    alerts_file: PathBuf,
}

impl TrafficAlertManager {
    fn new() -> Result<Self, String> {
        let config_dir =
            dirs::config_dir().ok_or_else(|| "Could not find config directory".to_string())?;

        let data_dir = config_dir.join("NetAssist");
        fs::create_dir_all(&data_dir)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;

        Ok(Self {
            alerts_file: data_dir.join("alerts.json"),
        })
    }

    /// Load alerts from file
    fn load_alerts(&self) -> Result<Vec<TrafficAlert>, String> {
        if self.alerts_file.exists() {
            let content = fs::read_to_string(&self.alerts_file)
                .map_err(|e| format!("Failed to read alerts file: {}", e))?;
            serde_json::from_str(&content)
                .map_err(|e| format!("Failed to parse alerts file: {}", e))
        } else {
            Ok(Self::default_alerts())
        }
    }

    /// Save alerts to file (atomic write)
    fn save_alerts(&self, alerts: &[TrafficAlert]) -> Result<(), String> {
        let content = serde_json::to_string_pretty(alerts)
            .map_err(|e| format!("Failed to serialize alerts: {}", e))?;
        let tmp = self.alerts_file.with_extension("tmp");
        fs::write(&tmp, &content)
            .map_err(|e| format!("Failed to write alerts file: {}", e))?;
        fs::rename(&tmp, &self.alerts_file)
            .map_err(|e| format!("Failed to finalize alerts file: {}", e))?;
        Ok(())
    }

    /// Get default alerts
    fn default_alerts() -> Vec<TrafficAlert> {
        vec![
            TrafficAlert {
                id: "daily_download".to_string(),
                name: "每日下载告警".to_string(),
                alert_type: "download".to_string(),
                threshold_bytes: 50 * 1024 * 1024 * 1024, // 50 GB
                period: "day".to_string(),
                enabled: true,
                triggered: false,
                last_triggered: None,
            },
            TrafficAlert {
                id: "daily_upload".to_string(),
                name: "每日上传告警".to_string(),
                alert_type: "upload".to_string(),
                threshold_bytes: 10 * 1024 * 1024 * 1024, // 10 GB
                period: "day".to_string(),
                enabled: true,
                triggered: false,
                last_triggered: None,
            },
            TrafficAlert {
                id: "monthly_total".to_string(),
                name: "每月总流量告警".to_string(),
                alert_type: "total".to_string(),
                threshold_bytes: 200 * 1024 * 1024 * 1024, // 200 GB
                period: "month".to_string(),
                enabled: true,
                triggered: false,
                last_triggered: None,
            },
        ]
    }

    /// Get all alerts
    fn get_alerts(&self) -> Result<Vec<TrafficAlert>, String> {
        self.load_alerts()
    }

    /// Update an alert
    fn update_alert(&self, alert: TrafficAlert) -> Result<(), String> {
        let mut alerts = self.load_alerts()?;
        let pos = alerts
            .iter()
            .position(|a| a.id == alert.id)
            .ok_or_else(|| format!("Alert not found: {}", alert.id))?;
        alerts[pos] = alert;
        self.save_alerts(&alerts)
    }

    /// Add a new alert
    fn add_alert(&self, alert: TrafficAlert) -> Result<(), String> {
        let mut alerts = self.load_alerts()?;
        // Check if ID already exists
        if alerts.iter().any(|a| a.id == alert.id) {
            return Err(format!("Alert with ID {} already exists", alert.id));
        }
        alerts.push(alert);
        self.save_alerts(&alerts)
    }

    /// Delete an alert
    fn delete_alert(&self, alert_id: &str) -> Result<(), String> {
        let mut alerts = self.load_alerts()?;
        let initial_len = alerts.len();
        alerts.retain(|a| a.id != alert_id);
        if alerts.len() == initial_len {
            return Err(format!("Alert not found: {}", alert_id));
        }
        self.save_alerts(&alerts)
    }

    /// Evaluate every enabled alert against its OWN period using the same
    /// counter-based cumulative method that the UI displays — so the number
    /// shown and the number that triggers an alert always agree. Persists the
    /// `triggered`/`last_triggered` bookkeeping.
    fn check_alerts(
        &self,
        storage: &mut TrafficHistoryStorage,
    ) -> Result<Vec<AlertStatus>, String> {
        let alerts = self.load_alerts()?;
        let mut statuses = Vec::new();
        let mut changed = false;

        for mut alert in alerts {
            if !alert.enabled {
                continue;
            }

            // Evaluate per-alert period (each alert has its own window).
            let cumulative = match storage.get_cumulative_traffic_via_counters(&alert.period) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(
                        "alert {} ({}) skipped: {}",
                        alert.name,
                        alert.period,
                        e
                    );
                    continue;
                }
            };

            let current_value = match alert.alert_type.as_str() {
                "download" => cumulative.total_download_bytes,
                "upload" => cumulative.total_upload_bytes,
                "total" => cumulative.total_download_bytes + cumulative.total_upload_bytes,
                _ => continue,
            };

            let triggered = current_value >= alert.threshold_bytes && alert.threshold_bytes > 0;
            let percentage = if alert.threshold_bytes > 0 {
                (current_value as f64 / alert.threshold_bytes as f64) * 100.0
            } else {
                0.0
            };

            // Track trigger state transitions.
            if triggered && !alert.triggered {
                alert.triggered = true;
                alert.last_triggered = Some(Local::now().timestamp_millis());
                changed = true;
            } else if !triggered && alert.triggered {
                alert.triggered = false;
                changed = true;
            }

            statuses.push(AlertStatus {
                alert_id: alert.id.clone(),
                triggered,
                current_value,
                threshold_value: alert.threshold_bytes,
                percentage,
            });

            if changed {
                changed = false;
                let _ = self.update_alert(alert);
            }
        }

        Ok(statuses)
    }
}

// Global instances (using std::sync::OnceLock instead of lazy_static)
use std::sync::OnceLock;

static HISTORY_STORAGE: OnceLock<std::sync::Mutex<TrafficHistoryStorage>> = OnceLock::new();
static ALERT_MANAGER: OnceLock<TrafficAlertManager> = OnceLock::new();

fn history_storage() -> &'static std::sync::Mutex<TrafficHistoryStorage> {
    HISTORY_STORAGE.get_or_init(|| {
        std::sync::Mutex::new(TrafficHistoryStorage::new().unwrap_or_else(|e| {
            tracing::error!("Failed to initialize traffic history storage: {}", e);
            TrafficHistoryStorage {
                data_dir: PathBuf::from("."),
                history_cache: HashMap::new(),
            }
        }))
    })
}

fn alert_manager() -> &'static TrafficAlertManager {
    ALERT_MANAGER.get_or_init(|| {
        TrafficAlertManager::new().unwrap_or_else(|e| {
            tracing::error!("Failed to initialize alert manager: {}", e);
            TrafficAlertManager {
                alerts_file: PathBuf::from("alerts.json"),
            }
        })
    })
}

/// Get cumulative traffic for a time period.
///
/// Prefers the OS interface-counter method (immediate, accurate) and falls
/// back to the per-minute history aggregation only if the counter method
/// errors. Both share the same period bounds via `period_bounds`.
#[tauri::command]
pub async fn get_cumulative_traffic(period: String) -> Result<CumulativeTraffic, String> {
    // File I/O inside mutex — run on blocking thread to avoid stalling async runtime
    tokio::task::spawn_blocking(move || {
        let mut storage = history_storage()
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        storage
            .get_cumulative_traffic_via_counters(&period)
            .or_else(|_| storage.get_cumulative_traffic(&period))
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Get traffic history for a time range (hours)
#[tauri::command]
pub async fn get_traffic_history(hours: i64) -> Result<TrafficHistory, String> {
    tokio::task::spawn_blocking(move || {
        let storage = history_storage()
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        storage.get_traffic_history(hours)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Record a traffic data point
#[tauri::command]
pub async fn record_traffic_point(download_bps: f64, upload_bps: f64) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let mut storage = history_storage()
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        storage.add_data_point(download_bps, upload_bps)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

/// Get all traffic alerts
#[tauri::command]
pub async fn get_traffic_alerts() -> Result<Vec<TrafficAlert>, String> {
    alert_manager().get_alerts()
}

/// Update a traffic alert
#[tauri::command]
pub async fn update_traffic_alert(alert: TrafficAlert) -> Result<(), String> {
    alert_manager().update_alert(alert)
}

/// Add a new traffic alert
#[tauri::command]
pub async fn add_traffic_alert(alert: TrafficAlert) -> Result<(), String> {
    alert_manager().add_alert(alert)
}

/// Delete a traffic alert
#[tauri::command]
pub async fn delete_traffic_alert(alert_id: String) -> Result<(), String> {
    alert_manager().delete_alert(&alert_id)
}

/// Check alert status.
///
/// The `period` argument is accepted for backward compatibility with the old
/// UI, but is intentionally ignored: every alert is evaluated against its own
/// `period` (see `TrafficAlertManager::check_alerts`).
#[tauri::command]
pub async fn check_traffic_alerts(_period: String) -> Result<Vec<AlertStatus>, String> {
    tokio::task::spawn_blocking(move || {
        let mut storage = history_storage()
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        alert_manager().check_alerts(&mut storage)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    /// Build a `TrafficHistoryStorage` rooted at a fresh temp dir so tests
    /// never touch the real `~/Library/Application Support/NetAssist` data.
    fn storage_in_tempdir() -> (TrafficHistoryStorage, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("create tempdir");
        let data_dir = dir.path().join("traffic");
        fs::create_dir_all(&data_dir).expect("create data_dir");
        let storage = TrafficHistoryStorage {
            data_dir,
            history_cache: HashMap::new(),
        };
        (storage, dir)
    }

    /// `period_bounds("day", ...)` should start at today's 00:00 local and end
    /// at `now`; the same invariant must hold for week (Monday) and month
    /// (1st). This is the shared foundation for both cumulative paths.
    #[test]
    fn test_period_bounds_day() {
        let now = Local::now();
        let (start, end, label) = TrafficHistoryStorage::period_bounds("day", now).unwrap();
        assert_eq!(label, "day");
        assert!(start <= end, "start must not be after end");
        assert_eq!(end, now.timestamp(), "end is the current timestamp");
        let start_dt = local_from_epoch(start).unwrap();
        assert_eq!(start_dt.hour(), 0);
        assert_eq!(start_dt.minute(), 0);
        assert_eq!(start_dt.second(), 0);
    }

    #[test]
    fn test_period_bounds_week_starts_monday() {
        let now = Local::now();
        let (start, _end, _label) = TrafficHistoryStorage::period_bounds("week", now).unwrap();
        let start_dt = local_from_epoch(start).unwrap();
        assert_eq!(
            start_dt.weekday().num_days_from_monday(),
            0,
            "week bound should fall on a Monday"
        );
        assert_eq!(start_dt.hour(), 0);
    }

    #[test]
    fn test_period_bounds_month_starts_first() {
        let now = Local::now();
        let (start, _end, _label) = TrafficHistoryStorage::period_bounds("month", now).unwrap();
        let start_dt = local_from_epoch(start).unwrap();
        assert_eq!(start_dt.day(), 1, "month bound should be the 1st");
        assert_eq!(start_dt.hour(), 0);
    }

    #[test]
    fn test_period_bounds_rejects_unknown_period() {
        let now = Local::now();
        assert!(TrafficHistoryStorage::period_bounds("year", now).is_err());
        assert!(TrafficHistoryStorage::period_bounds("", now).is_err());
    }

    /// First call for a period must create an anchor at the current OS
    /// counters and report 0 bytes consumed (baseline == current).
    #[test]
    fn test_anchor_initial_baselines_to_zero() {
        let (mut storage, _dir) = storage_in_tempdir();

        let result = storage
            .cumulative_from_counters("day", 1_000_000, 2_000_000)
            .expect("counter cumulative succeeds");

        assert_eq!(result.total_download_bytes, 0, "first read is 0");
        assert_eq!(result.total_upload_bytes, 0, "first read is 0");
        assert_eq!(result.period, "day");

        let anchors = storage.load_anchors();
        assert_ne!(anchors.day.start_ts, 0, "anchor start_ts was written");
        assert_eq!(anchors.day.last_rx, 1_000_000);
        assert_eq!(anchors.day.last_tx, 2_000_000);
    }

    /// A pre-existing anchor with a stale `start_ts` (period rolled over)
    /// must be re-baselined, so the cumulative total restarts from 0.
    #[test]
    fn test_anchor_period_rollover_rebaselines() {
        let (mut storage, _dir) = storage_in_tempdir();

        let stale = AnchorStore {
            day: PeriodAnchor {
                start_ts: 946_684_800, // 2000-01-01T00:00:00Z
                last_rx: 0,
                last_tx: 0,
                accrued_rx: 5_000,
                accrued_tx: 5_000,
            },
            ..Default::default()
        };
        storage.save_anchors(&stale);

        let result = storage
            .cumulative_from_counters("day", 1_000_000, 1_000_000)
            .expect("counter cumulative succeeds");

        // New period → accumulator reset to 0.
        assert_eq!(result.total_download_bytes, 0);
        assert_eq!(result.total_upload_bytes, 0);

        let anchors = storage.load_anchors();
        let (expected_start, _, _) =
            TrafficHistoryStorage::period_bounds("day", Local::now()).unwrap();
        assert_eq!(anchors.day.start_ts, expected_start);
        assert_eq!(anchors.day.accrued_rx, 0);
    }

    /// Monotonic counter growth accrues the delta on every read.
    #[test]
    fn test_anchor_accrues_forward_growth() {
        let (mut storage, _dir) = storage_in_tempdir();

        // First read at 1_000_000 → anchor, 0 accrued.
        let r1 = storage
            .cumulative_from_counters("day", 1_000_000, 2_000_000)
            .unwrap();
        assert_eq!(r1.total_download_bytes, 0);

        // Second read 100 bytes later → accrued 100.
        let r2 = storage
            .cumulative_from_counters("day", 1_000_100, 2_000_500)
            .unwrap();
        assert_eq!(r2.total_download_bytes, 100);
        assert_eq!(r2.total_upload_bytes, 500);

        // Third read another 50 later → accrued 150.
        let r3 = storage
            .cumulative_from_counters("day", 1_000_150, 2_000_500)
            .unwrap();
        assert_eq!(r3.total_download_bytes, 150);
        assert_eq!(r3.total_upload_bytes, 500);
    }

    /// When the OS counters go *backwards* vs the anchor (interface reset /
    /// reboot / Wi-Fi switch / container removal), already-accrued traffic is
    /// preserved — the total must NOT jump to 0, and later growth resumes.
    #[test]
    fn test_anchor_counter_reset_preserves_accrued() {
        let (mut storage, _dir) = storage_in_tempdir();

        let r1 = storage
            .cumulative_from_counters("day", 1_000_000, 1_000_000)
            .unwrap();
        assert_eq!(r1.total_download_bytes, 0);

        // Grow to 1_000_100 (accrued 100).
        storage
            .cumulative_from_counters("day", 1_000_100, 1_000_000)
            .unwrap();

        // Counters reset to a small value (interface flap / reboot).
        let r3 = storage
            .cumulative_from_counters("day", 100, 50)
            .unwrap();
        assert_eq!(
            r3.total_download_bytes, 100,
            "accrued survives a counter reset"
        );
        assert_eq!(r3.total_upload_bytes, 0);

        // New traffic after reset accrues from the new anchor.
        let r4 = storage
            .cumulative_from_counters("day", 200, 150)
            .unwrap();
        assert_eq!(r4.total_download_bytes, 200, "100 old + 100 new");
        assert_eq!(r4.total_upload_bytes, 100);
    }

    /// AnchorStore must round-trip through JSON so the persisted baseline
    /// survives app restarts.
    #[test]
    fn test_anchor_store_serde_roundtrip() {
        let store = AnchorStore {
            day: PeriodAnchor {
                start_ts: 1_700_000_000,
                last_rx: 1234,
                last_tx: 5678,
                accrued_rx: 90,
                accrued_tx: 12,
            },
            week: PeriodAnchor {
                start_ts: 1_700_000_000,
                last_rx: 9999,
                last_tx: 0,
                accrued_rx: 0,
                accrued_tx: 0,
            },
            month: PeriodAnchor::default(),
        };

        let json = serde_json::to_string(&store).unwrap();
        let back: AnchorStore = serde_json::from_str(&json).unwrap();
        assert_eq!(back.day.last_rx, 1234);
        assert_eq!(back.day.last_tx, 5678);
        assert_eq!(back.day.accrued_rx, 90);
        assert_eq!(back.week.last_rx, 9999);
        assert_eq!(back.month.start_ts, 0);
    }

    /// A corrupt/empty anchors.json must degrade to the default store rather
    /// than poisoning the page (the whole point of the fallback).
    #[test]
    fn test_load_anchors_tolerates_corrupt_file() {
        let (storage, _dir) = storage_in_tempdir();
        fs::write(storage.anchors_file_path(), "not valid json {{{").unwrap();

        let anchors = storage.load_anchors();
        assert_eq!(anchors.day.start_ts, 0, "corrupt file -> default anchor");
    }

    /// Old anchor files from previous versions serialize differently and must
    /// not crash loading (missing fields get Default, extra fields ignored).
    #[test]
    fn test_anchor_load_tolerates_unknown_json() {
        let (storage, _dir) = storage_in_tempdir();
        fs::write(
            storage.anchors_file_path(),
            r#"{"day":{"start_ts":123,"rx_bytes":1,"tx_bytes":2,"carried_rx":3}}"#,
        )
        .unwrap();
        // Default deny-unknown-fields behavior would error; we use a relaxed
        // load that falls back to defaults on any parse failure.
        let anchors = storage.load_anchors();
        assert_eq!(anchors.day.start_ts, 0, "mismatched schema -> defaults");
    }
}
