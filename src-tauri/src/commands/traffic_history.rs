//! Traffic history and alert management commands

use crate::models::{
    AlertStatus, CumulativeTraffic, TrafficAlert, TrafficHistory, TrafficHistoryPoint,
};
use chrono::{DateTime, Datelike, Utc};
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

        // Keep only recent data points (last 24 hours worth, one per minute)
        let max_points = 24 * 60;
        if day_data.len() > max_points {
            day_data = day_data.into_iter().rev().take(max_points).collect();
            day_data.sort_by_key(|p| p.timestamp);
        }

        self.save_day_history(&date_str, &day_data)?;
        self.history_cache.insert(date_str, day_data);

        Ok(())
    }

    /// Get cumulative traffic for a time period
    fn get_cumulative_traffic(&self, period: &str) -> Result<CumulativeTraffic, String> {
        let now = Utc::now();
        let (start_time, end_time, period_label) = match period {
            "day" => {
                let start = now
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .and_then(|d| d.and_local_timezone(Utc).single())
                    .unwrap_or_else(|| Utc::now());
                (start.timestamp(), now.timestamp(), "day".to_string())
            }
            "week" => {
                let weekday = now.weekday().num_days_from_monday();
                let start = now
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .and_then(|d| d.and_local_timezone(Utc).single())
                    .unwrap_or_else(|| Utc::now());
                let start = start - chrono::Duration::days(weekday as i64);
                (start.timestamp(), now.timestamp(), "week".to_string())
            }
            "month" => {
                let start = now
                    .date_naive()
                    .with_day(1)
                    .and_then(|d| d.and_hms_opt(0, 0, 0))
                    .and_then(|d| d.and_local_timezone(Utc).single())
                    .unwrap_or_else(|| Utc::now());
                (start.timestamp(), now.timestamp(), "month".to_string())
            }
            _ => return Err(format!("Invalid period: {}", period)),
        };

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

/// Get cumulative traffic for a time period
#[tauri::command]
pub async fn get_cumulative_traffic(period: String) -> Result<CumulativeTraffic, String> {
    // File I/O inside mutex — run on blocking thread to avoid stalling async runtime
    tokio::task::spawn_blocking(move || {
        let storage = history_storage()
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        storage.get_cumulative_traffic(&period)
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
