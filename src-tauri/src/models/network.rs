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

/// Outcome of a location lookup, independent of network connectivity.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum GeoIPStatus {
    Success,
    Local,
    NoData,
    #[default]
    Failed,
}

/// GeoIP location information and the evidence behind it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeoIPInfo {
    pub country: String,
    pub region: String,
    pub city: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub status: GeoIPStatus,
    /// The address looked up, never the location provider's observed client IP.
    pub ip: String,
    pub provider: Option<String>,
    /// Time of the actual query, preserved when serving cached results.
    pub queried_at: i64,
    pub error_kind: Option<String>,
    pub error: Option<String>,
}

impl Default for GeoIPInfo {
    fn default() -> Self {
        Self {
            country: "-".to_string(),
            region: "-".to_string(),
            city: "-".to_string(),
            latitude: None,
            longitude: None,
            status: GeoIPStatus::Failed,
            ip: String::new(),
            provider: None,
            queried_at: 0,
            error_kind: None,
            error: None,
        }
    }
}

/// IP address information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IPInfo {
    /// Public IPv4 address (external, visible from internet)
    pub ipv4: Option<String>,
    /// First eligible local IPv6 configuration; not a measured public egress.
    pub ipv6: Option<String>,
    /// Local IPv4 address (internal/LAN)
    pub local_ipv4: Option<String>,
    /// Local IPv6 address (internal/LAN)
    pub local_ipv6: Option<String>,
    pub ipv6_interface: Option<String>,
    pub ipv6_source: Option<String>,
    pub local_addresses: Vec<LocalAddress>,
    pub public_ipv4_probe: PublicIpProbe,
    pub ipv4_type: IPType,
    pub ipv6_type: IPType,
    pub ipv4_geoip: Option<GeoIPInfo>,
    pub ipv6_geoip: Option<GeoIPInfo>,
    /// Both address families are configured; does not establish internet reachability.
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
            ipv6_interface: None,
            ipv6_source: None,
            local_addresses: Vec::new(),
            public_ipv4_probe: PublicIpProbe::default(),
            ipv4_type: IPType::Unknown,
            ipv6_type: IPType::Unknown,
            ipv4_geoip: None,
            ipv6_geoip: None,
            dual_stack_enabled: false,
            ipv6_priority: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalAddress {
    pub address: String,
    pub interface: Option<String>,
    pub family: String,
    pub source: String,
}

/// HTTP proxy bypass does not bypass a VPN/TUN or the operating system's routes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicIpProbe {
    pub status: String,
    pub url: Option<String>,
    pub error: Option<String>,
    pub proxy_policy: String,
    pub checked_at: Option<i64>,
    pub address_family: Option<String>,
    pub cache_hit: bool,
}

impl Default for PublicIpProbe {
    fn default() -> Self {
        Self {
            status: "skipped".to_string(),
            url: None,
            error: None,
            proxy_policy: "no_proxy".to_string(),
            checked_at: None,
            address_family: None,
            cache_hit: false,
        }
    }
}

/// Evidence for one HTTP target, not a claim about other applications or routes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpProbeResult {
    pub url: String,
    pub success: bool,
    pub latency_ms: f64,
    pub status_code: Option<u16>,
    pub error: Option<String>,
    pub error_kind: Option<String>,
    pub proxy_policy: String,
    pub checked_at: i64,
    /// The connected peer's family when available; never guessed from local addresses.
    pub address_family: Option<String>,
}

/// Overall network status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStatus {
    /// Overall status: "normal" or "abnormal"
    pub status: String,
    pub message: String,
    pub timestamp: i64,
    pub probes: Vec<HttpProbeResult>,
}

impl Default for NetworkStatus {
    fn default() -> Self {
        Self {
            status: "unknown".to_string(),
            message: "检测中...".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            probes: Vec::new(),
        }
    }
}
