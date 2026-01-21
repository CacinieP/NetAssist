// Data models for network monitoring

pub mod network;
pub mod traffic;
pub mod dns;
pub mod connection;
pub mod diagnostic;

// Re-export commonly used types
pub use network::{IPInfo, IPType, GeoIPInfo, NetworkStatus};
pub use traffic::{TrafficStats, AppTraffic, CumulativeTraffic, TrafficHistory, TrafficHistoryPoint, TrafficAlert, AlertStatus};
pub use dns::DNSStats;
pub use connection::{ConnectionInfo, ConnectionState};
pub use diagnostic::{DiagnosticResult, DiagnosticItem, DiagnosticStatus, RepairAction, RepairType};
