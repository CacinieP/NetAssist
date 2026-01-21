use serde::{Deserialize, Serialize};

use super::network::GeoIPInfo;

/// Connection state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConnectionState {
    Established,
    Listening,
    TimeWait,
    CloseWait,
    Unknown,
}

/// Network connection information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    /// Local process ID
    pub pid: u32,
    /// Process name
    pub process_name: String,
    /// Protocol (TCP/UDP)
    pub protocol: String,
    /// Local address
    pub local_address: String,
    /// Local port
    pub local_port: u16,
    /// Remote address
    pub remote_address: String,
    /// Remote port
    pub remote_port: u16,
    /// Connection state
    pub state: ConnectionState,
    /// GeoIP information for remote address
    pub remote_geoip: Option<GeoIPInfo>,
    /// Current data rate in bytes per second
    pub data_rate_bps: f64,
    /// Total bytes transferred
    pub total_bytes: u64,
}
