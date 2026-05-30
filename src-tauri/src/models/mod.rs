// Data models for network monitoring

pub mod connection;
pub mod diagnostic;
pub mod dns;
pub mod network;
pub mod traffic;

// Re-export commonly used types
pub use connection::{ConnectionInfo, ConnectionState};
pub use diagnostic::{
    DiagnosticItem, DiagnosticResult, DiagnosticStatus, RepairAction, RepairType,
};
pub use dns::DNSStats;
pub use network::{GeoIPInfo, IPInfo, IPType, NetworkStatus};
pub use traffic::{
    AlertStatus, AppTraffic, CumulativeTraffic, TrafficAlert, TrafficHistory, TrafficHistoryPoint,
    TrafficStats,
};
