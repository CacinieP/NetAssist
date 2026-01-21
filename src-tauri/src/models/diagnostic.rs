use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Diagnostic status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticStatus {
    Pass,
    Fail,
    Warning,
}

/// Individual diagnostic item result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticItem {
    pub status: DiagnosticStatus,
    pub message: String,
    pub details: Value,
    pub duration_ms: u64,
}

/// Repair action type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RepairType {
    #[serde(rename = "reset_network_stack")]
    ResetNetworkStack,
    #[serde(rename = "switch_dns")]
    SwitchDNS,
    #[serde(rename = "toggle_ipv6")]
    ToggleIPv6,
    #[serde(rename = "disconnect_abnormal")]
    DisconnectAbnormal,
    #[serde(rename = "release_renew_ip")]
    ReleaseRenewIP,
    #[serde(rename = "flush_dns_cache")]
    FlushDNSCache,
    #[serde(rename = "reset_adapter")]
    ResetAdapter,
    #[serde(rename = "restart_network_service")]
    RestartNetworkService,
}

/// Recommended repair action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairAction {
    pub action_type: RepairType,
    pub name: String,
    pub description: String,
    pub priority: u8, // 1-10, lower is higher priority
    pub estimated_time_seconds: u64,
}

/// Complete diagnostic result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticResult {
    pub overall_status: DiagnosticStatus,
    pub network_connectivity: DiagnosticItem,
    pub ip_configuration: DiagnosticItem,
    pub dns_resolution: DiagnosticItem,
    pub network_quality: DiagnosticItem,
    pub recommendations: Vec<RepairAction>,
    pub timestamp: i64,
}
