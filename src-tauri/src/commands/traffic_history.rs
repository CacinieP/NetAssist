//! Traffic history and alert management commands

use crate::models::{
    AlertStatus, CumulativeTraffic, TrafficAlert, TrafficHistory, TrafficHistoryPoint,
};
use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

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
        if year < 2000 || year > 2100 {
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

    /// Save traffic history for a specific date
    fn save_day_history(&self, date_str: &str, data: &[TrafficHistoryPoint]) -> Result<(), String> {
        let file_path = self.get_date_file_path(date_str)?;
        let content = serde_json::to_string_pretty(data)
            .map_err(|e| format!("Failed to serialize history: {}", e))?;
        fs::write(&file_path, content)
            .map_err(|e| format!("Failed to write history file: {}", e))?;
        Ok(())
    }

    /// Add a new traffic data point
    fn add_data_point(&mut self, download_bps: f64, upload_bps: f64) -> Result<(), String> {
        let now = Utc::now();
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
        // The recording interval is configurable (frontend default 5s), so a
        // fixed point cap would either over- or under-trim. Dropping anything
        // older than 24h keeps each daily file bounded to one day and matches
        // the largest trend-chart range that reads a single day.
        let cutoff_ms = now.timestamp_millis() - 24 * 60 * 60 * 1000;
        day_data.retain(|p| p.timestamp >= cutoff_ms);
        day_data.sort_by_key(|p| p.timestamp);

        self.save_day_history(&date_str, &day_data)?;
        self.history_cache.insert(date_str, day_data);

        Ok(())
    }

    /// Get cumulative traffic for a time period
    fn get_cumulative_traffic(&self, period: &str) -> Result<CumulativeTraffic, String> {
        let now = Utc::now();
        let (start_time, end_time, period_label) = Self::period_bounds(period, now)?;

        let mut total_download = 0u64;
        let mut total_upload = 0u64;

        // Load data for each day in the period
        let start_date = DateTime::from_timestamp(start_time, 0).unwrap_or_else(|| Utc::now());
        let end_date = DateTime::from_timestamp(end_time, 0).unwrap_or_else(|| Utc::now());
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
                    if point.timestamp >= start_time_ms && point.timestamp <= end_time_ms {
                        // Calculate time interval to next point (or use 60s as default)
                        let interval_seconds = if i + 1 < sorted_data.len() {
                            ((sorted_data[i + 1].timestamp - point.timestamp) as f64) / 1000.0
                        } else {
                            60.0 // Default recording interval for last point
                        };

                        // Calculate actual bytes: rate (bps) * time interval (seconds)
                        total_download += (point.download_bps * interval_seconds) as u64;
                        total_upload += (point.upload_bps * interval_seconds) as u64;
                    }
                }
            }
            current_date = current_date + chrono::Duration::days(1);
        }

        Ok(CumulativeTraffic {
            total_download_bytes: total_download,
            total_upload_bytes: total_upload,
            start_timestamp: start_time,
            end_timestamp: end_time,
            period: period_label,
        })
    }

    /// Get traffic history for a time range
    fn get_traffic_history(&self, hours: i64) -> Result<TrafficHistory, String> {
        let now = Utc::now();
        let start_time = now - chrono::Duration::hours(hours);

        let mut all_data = Vec::new();

        let start_date =
            DateTime::from_timestamp(start_time.timestamp(), 0).unwrap_or_else(|| Utc::now());
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
            current_date = current_date + chrono::Duration::days(1);
        }

        all_data.sort_by_key(|p| p.timestamp);

        Ok(TrafficHistory {
            data: all_data,
            start_timestamp: start_time.timestamp_millis(),
            end_timestamp: now.timestamp_millis(),
        })
    }

    /// Compute the (start, end, label) bounds for a period relative to `now`.
    /// Extracted so it can be shared by the file-based and counter-based
    /// cumulative calculations and unit-tested in isolation.
    fn period_bounds(
        period: &str,
        now: DateTime<Utc>,
    ) -> Result<(i64, i64, String), String> {
        match period {
            "day" => {
                let start = now
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .and_then(|d| d.and_local_timezone(Utc).single())
                    .unwrap_or(now);
                Ok((start.timestamp(), now.timestamp(), "day".to_string()))
            }
            "week" => {
                let weekday = now.weekday().num_days_from_monday();
                let start = now
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .and_then(|d| d.and_local_timezone(Utc).single())
                    .unwrap_or(now);
                let start = start - chrono::Duration::days(weekday as i64);
                Ok((start.timestamp(), now.timestamp(), "week".to_string()))
            }
            "month" => {
                let start = now
                    .date_naive()
                    .with_day(1)
                    .and_then(|d| d.and_hms_opt(0, 0, 0))
                    .and_then(|d| d.and_local_timezone(Utc).single())
                    .unwrap_or(now);
                Ok((start.timestamp(), now.timestamp(), "month".to_string()))
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
            if let Err(e) = fs::write(&path, content) {
                tracing::warn!("Failed to write traffic anchors file: {}", e);
            }
        }
    }

    /// Cumulative traffic driven by the OS interface byte counters.
    ///
    /// On the first request for a period (or when the period has rolled over)
    /// we snapshot the current interface counters as the period's "start
    /// anchor" and persist it. The reported total is
    /// `current_counters − anchor_counters`. When the counters reset below the
    /// anchor (interface flap / reboot / Wi-Fi switch) we re-baseline to the
    /// current value so the period restarts from 0 instead of going negative.
    ///
    /// This gives immediate, accurate, OS-authoritative totals — independent
    /// of the per-minute history sampling — so the page shows real numbers the
    /// instant it is opened instead of waiting minutes for samples to accrue.
    fn get_cumulative_traffic_via_counters(
        &mut self,
        period: &str,
    ) -> Result<CumulativeTraffic, String> {
        let now = Utc::now();
        let (start_ts, end_ts, label) = Self::period_bounds(period, now)?;

        let (current_rx, current_tx) = crate::platform::get_interface_total_bytes();

        let mut anchors = self.load_anchors();

        // Resolve the anchor for this period and re-baseline when needed. We
        // touch the field directly (rather than via a mut ref helper) so the
        // borrow of `anchors` ends before we persist it.
        let anchor = match period {
            "week" => &mut anchors.week,
            "month" => &mut anchors.month,
            // default to day for "day" and any unrecognized value
            _ => &mut anchors.day,
        };

        // Period rollover (new day/week/month) or counter reset (interface
        // flap/reboot) both require re-baselining to the current counters.
        let needs_reset = anchor.start_ts != start_ts
            || current_rx < anchor.rx_bytes
            || current_tx < anchor.tx_bytes;

        if needs_reset {
            anchor.start_ts = start_ts;
            anchor.rx_bytes = current_rx;
            anchor.tx_bytes = current_tx;
        }

        // Snapshot the anchor baseline before the immutable save borrow.
        let baseline_rx = anchor.rx_bytes;
        let baseline_tx = anchor.tx_bytes;

        if needs_reset {
            self.save_anchors(&anchors);
        }

        let total_download = current_rx.saturating_sub(baseline_rx);
        let total_upload = current_tx.saturating_sub(baseline_tx);

        Ok(CumulativeTraffic {
            total_download_bytes: total_download,
            total_upload_bytes: total_upload,
            start_timestamp: start_ts,
            end_timestamp: end_ts,
            period: label,
        })
    }
}

/// Snapshot of the interface byte counters captured at the start of a period.
/// `start_ts` lets us detect period rollover (new day/week/month).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PeriodAnchor {
    start_ts: i64,
    rx_bytes: u64,
    tx_bytes: u64,
}

impl Default for PeriodAnchor {
    fn default() -> Self {
        Self {
            start_ts: 0,
            rx_bytes: 0,
            tx_bytes: 0,
        }
    }
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

    /// Save alerts to file
    fn save_alerts(&self, alerts: &[TrafficAlert]) -> Result<(), String> {
        let content = serde_json::to_string_pretty(alerts)
            .map_err(|e| format!("Failed to serialize alerts: {}", e))?;
        fs::write(&self.alerts_file, content)
            .map_err(|e| format!("Failed to write alerts file: {}", e))?;
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

    /// Check alert status
    fn check_alerts(&self, cumulative: &CumulativeTraffic) -> Result<Vec<AlertStatus>, String> {
        let alerts = self.load_alerts()?;
        let mut statuses = Vec::new();

        for alert in alerts {
            if !alert.enabled {
                continue;
            }

            let current_value = match alert.alert_type.as_str() {
                "download" => cumulative.total_download_bytes,
                "upload" => cumulative.total_upload_bytes,
                "total" => cumulative.total_download_bytes + cumulative.total_upload_bytes,
                _ => continue,
            };

            let triggered = current_value >= alert.threshold_bytes;
            let percentage = (current_value as f64 / alert.threshold_bytes as f64) * 100.0;

            statuses.push(AlertStatus {
                alert_id: alert.id.clone(),
                triggered,
                current_value,
                threshold_value: alert.threshold_bytes,
                percentage,
            });
        }

        // Also honor the user's `traffic_limit_gb` setting from Settings as an
        // additional "total" alert, so the 流量限制 field actually drives an
        // alert. Failures to load settings are non-fatal (just skip it).
        if let Ok(settings) = crate::commands::settings::load_settings_from_file() {
            if settings.traffic_limit_gb > 0.0 {
                let threshold_bytes = (settings.traffic_limit_gb * 1024.0 * 1024.0 * 1024.0) as u64;
                let current_value =
                    cumulative.total_download_bytes + cumulative.total_upload_bytes;
                let triggered = current_value >= threshold_bytes;
                let percentage = if threshold_bytes > 0 {
                    (current_value as f64 / threshold_bytes as f64) * 100.0
                } else {
                    0.0
                };
                statuses.push(AlertStatus {
                    alert_id: "traffic_limit_gb".to_string(),
                    triggered,
                    current_value,
                    threshold_value: threshold_bytes,
                    percentage,
                });
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

/// Check alert status
#[tauri::command]
pub async fn check_traffic_alerts(period: String) -> Result<Vec<AlertStatus>, String> {
    tokio::task::spawn_blocking(move || {
        let storage = history_storage()
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        let cumulative = storage.get_cumulative_traffic(&period)?;
        alert_manager().check_alerts(&cumulative)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

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

    /// `period_bounds("day", ...)` should start at today's 00:00 UTC and end
    /// at `now`; the same invariant must hold for week (Monday) and month
    /// (1st). This is the shared foundation for both cumulative paths.
    #[test]
    fn test_period_bounds_day() {
        let now = Utc::now();
        let (start, end, label) = TrafficHistoryStorage::period_bounds("day", now).unwrap();
        assert_eq!(label, "day");
        assert!(start <= end, "start must not be after end");
        assert_eq!(end, now.timestamp(), "end is the current timestamp");
        // start should be at or after 00:00 today.
        let start_dt = DateTime::<Utc>::from_timestamp(start, 0).unwrap();
        assert_eq!(start_dt.hour(), 0);
        assert_eq!(start_dt.minute(), 0);
        assert_eq!(start_dt.second(), 0);
    }

    #[test]
    fn test_period_bounds_week_starts_monday() {
        let now = Utc::now();
        let (start, _end, _label) = TrafficHistoryStorage::period_bounds("week", now).unwrap();
        let start_dt = DateTime::<Utc>::from_timestamp(start, 0).unwrap();
        // Monday in chrono's weekday() is 0 (num_days_from_monday).
        assert_eq!(
            start_dt.weekday().num_days_from_monday(),
            0,
            "week bound should fall on a Monday"
        );
        assert_eq!(start_dt.hour(), 0);
    }

    #[test]
    fn test_period_bounds_month_starts_first() {
        let now = Utc::now();
        let (start, _end, _label) =
            TrafficHistoryStorage::period_bounds("month", now).unwrap();
        let start_dt = DateTime::<Utc>::from_timestamp(start, 0).unwrap();
        assert_eq!(start_dt.day(), 1, "month bound should be the 1st");
        assert_eq!(start_dt.hour(), 0);
    }

    #[test]
    fn test_period_bounds_rejects_unknown_period() {
        let now = Utc::now();
        assert!(TrafficHistoryStorage::period_bounds("year", now).is_err());
        assert!(TrafficHistoryStorage::period_bounds("", now).is_err());
    }

    /// First call for a period must create an anchor at the current OS
    /// counters and report 0 bytes consumed (baseline == current).
    #[test]
    fn test_anchor_initial_baselines_to_zero() {
        let (mut storage, _dir) = storage_in_tempdir();

        let result = storage
            .get_cumulative_traffic_via_counters("day")
            .expect("counter cumulative succeeds");

        assert_eq!(result.total_download_bytes, 0, "first read is 0");
        assert_eq!(result.total_upload_bytes, 0, "first read is 0");
        assert_eq!(result.period, "day");

        // The anchors file must now exist and carry the day anchor.
        let anchors = storage.load_anchors();
        assert_ne!(anchors.day.start_ts, 0, "anchor start_ts was written");
    }

    /// A pre-existing anchor with a stale `start_ts` (period rolled over)
    /// must be re-baselined, so the cumulative total restarts from 0.
    #[test]
    fn test_anchor_period_rollover_rebaselines() {
        let (mut storage, _dir) = storage_in_tempdir();

        // Seed an obviously-stale anchor (year 2000) so the period check trips.
        let stale = AnchorStore {
            day: PeriodAnchor {
                start_ts: 946_684_800, // 2000-01-01T00:00:00Z
                rx_bytes: 0,
                tx_bytes: 0,
            },
            ..Default::default()
        };
        storage.save_anchors(&stale);

        let result = storage
            .get_cumulative_traffic_via_counters("day")
            .expect("counter cumulative succeeds");

        // After re-baseline the total is current − current == 0.
        assert_eq!(result.total_download_bytes, 0);
        assert_eq!(result.total_upload_bytes, 0);

        // The persisted anchor must now reference this period's start.
        let anchors = storage.load_anchors();
        let (expected_start, _, _) =
            TrafficHistoryStorage::period_bounds("day", Utc::now()).unwrap();
        assert_eq!(anchors.day.start_ts, expected_start);
    }

    /// When the OS counters go *backwards* vs the anchor (interface reset /
    /// reboot / Wi-Fi switch), we must re-baseline instead of underflowing.
    #[test]
    fn test_anchor_counter_reset_rebaselines() {
        let (mut storage, _dir) = storage_in_tempdir();

        // Anchor at a huge value; the real OS counters will be far below it,
        // simulating a reset. start_ts is forced to match this period so the
        // *only* trip is the backwards-counter check.
        let (start_ts, _, _) = TrafficHistoryStorage::period_bounds("day", Utc::now()).unwrap();
        let inflated = AnchorStore {
            day: PeriodAnchor {
                start_ts,
                rx_bytes: u64::MAX / 2,
                tx_bytes: u64::MAX / 2,
            },
            ..Default::default()
        };
        storage.save_anchors(&inflated);

        let result = storage
            .get_cumulative_traffic_via_counters("day")
            .expect("counter cumulative succeeds");

        // No underflow: the result is 0 (re-baselined) rather than a wrapped
        // huge number from saturating_sub(u64::MAX/2 - current).
        assert!(
            result.total_download_bytes < u64::MAX / 4,
            "reset path must not report a wrapped/huge value"
        );
        assert!(result.total_upload_bytes < u64::MAX / 4);

        // Anchor counters were rewritten down to the real OS counters.
        let anchors = storage.load_anchors();
        assert!(anchors.day.rx_bytes < u64::MAX / 2);
        assert!(anchors.day.tx_bytes < u64::MAX / 2);
    }

    /// AnchorStore must round-trip through JSON so the persisted baseline
    /// survives app restarts.
    #[test]
    fn test_anchor_store_serde_roundtrip() {
        let store = AnchorStore {
            day: PeriodAnchor {
                start_ts: 1_700_000_000,
                rx_bytes: 1234,
                tx_bytes: 5678,
            },
            week: PeriodAnchor {
                start_ts: 1_700_000_000,
                rx_bytes: 9999,
                tx_bytes: 0,
            },
            month: PeriodAnchor::default(),
        };

        let json = serde_json::to_string(&store).unwrap();
        let back: AnchorStore = serde_json::from_str(&json).unwrap();
        assert_eq!(back.day.rx_bytes, 1234);
        assert_eq!(back.day.tx_bytes, 5678);
        assert_eq!(back.week.rx_bytes, 9999);
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
}
