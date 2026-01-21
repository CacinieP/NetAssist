use serde::{Deserialize, Serialize};

/// Real-time traffic statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficStats {
    /// Download speed in bytes per second
    pub download_bps: f64,
    /// Upload speed in bytes per second
    pub upload_bps: f64,
    pub timestamp: i64,
}

impl Default for TrafficStats {
    fn default() -> Self {
        Self {
            download_bps: 0.0,
            upload_bps: 0.0,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }
}

/// Application traffic information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppTraffic {
    /// Application name
    pub name: String,
    /// Process ID
    pub pid: u32,
    /// Total download bytes
    pub download_bytes: u64,
    /// Total upload bytes
    pub upload_bytes: u64,
    /// Current download speed in bytes per second
    pub current_download_bps: f64,
    /// Current upload speed in bytes per second
    pub current_upload_bps: f64,
}

/// Cumulative traffic statistics for a time period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CumulativeTraffic {
    /// Total download bytes
    pub total_download_bytes: u64,
    /// Total upload bytes
    pub total_upload_bytes: u64,
    /// Start timestamp
    pub start_timestamp: i64,
    /// End timestamp
    pub end_timestamp: i64,
    /// Time period type: "day", "week", "month"
    pub period: String,
}

/// Traffic history data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficHistoryPoint {
    /// Timestamp
    pub timestamp: i64,
    /// Download speed in bytes per second
    pub download_bps: f64,
    /// Upload speed in bytes per second
    pub upload_bps: f64,
}

/// Traffic history for a time range
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficHistory {
    /// History data points
    pub data: Vec<TrafficHistoryPoint>,
    /// Start timestamp
    pub start_timestamp: i64,
    /// End timestamp
    pub end_timestamp: i64,
}

/// Traffic alert configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficAlert {
    /// Alert ID
    pub id: String,
    /// Alert name
    pub name: String,
    /// Alert type: "download", "upload", "total"
    pub alert_type: String,
    /// Threshold value in bytes
    pub threshold_bytes: u64,
    /// Time period: "hour", "day", "week", "month"
    pub period: String,
    /// Whether alert is enabled
    pub enabled: bool,
    /// Whether alert has been triggered
    pub triggered: bool,
    /// Last triggered timestamp
    pub last_triggered: Option<i64>,
}

/// Traffic alert status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertStatus {
    /// Alert ID
    pub alert_id: String,
    /// Whether alert is currently triggered
    pub triggered: bool,
    /// Current value
    pub current_value: u64,
    /// Threshold value
    pub threshold_value: u64,
    /// Percentage of threshold used
    pub percentage: f64,
}
