use serde::{Deserialize, Serialize};

/// IP address type classification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IPType {
    Public,
    Private,
    LinkLocal,
    Loopback,
    Global,
    Unknown,
}

/// GeoIP location information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoIPInfo {
    pub country: String,
    pub region: String,
    pub city: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

impl Default for GeoIPInfo {
    fn default() -> Self {
        Self {
            country: "未知".to_string(),
            region: "-".to_string(),
            city: "-".to_string(),
            latitude: None,
            longitude: None,
        }
    }
}

/// IP address information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IPInfo {
    /// Public IPv4 address (external, visible from internet)
    pub ipv4: Option<String>,
    /// Public IPv6 address (external, visible from internet)
    pub ipv6: Option<String>,
    /// Local IPv4 address (internal/LAN)
    pub local_ipv4: Option<String>,
    /// Local IPv6 address (internal/LAN)
    pub local_ipv6: Option<String>,
    pub ipv4_type: IPType,
    pub ipv6_type: IPType,
    pub ipv4_geoip: Option<GeoIPInfo>,
    pub ipv6_geoip: Option<GeoIPInfo>,
    pub dual_stack_enabled: bool,
    pub ipv6_priority: bool,
}

impl Default for IPInfo {
    fn default() -> Self {
        Self {
            ipv4: None,
            ipv6: None,
            local_ipv4: None,
            local_ipv6: None,
            ipv4_type: IPType::Unknown,
            ipv6_type: IPType::Unknown,
            ipv4_geoip: None,
            ipv6_geoip: None,
            dual_stack_enabled: false,
            ipv6_priority: false,
        }
    }
}

/// Overall network status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStatus {
    /// Overall status: "normal" or "abnormal"
    pub status: String,
    pub message: String,
    pub timestamp: i64,
}

impl Default for NetworkStatus {
    fn default() -> Self {
        Self {
            status: "unknown".to_string(),
            message: "检测中...".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }
}
