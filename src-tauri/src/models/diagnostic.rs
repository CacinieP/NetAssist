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
    #[serde(rename = "restore_auto_dns")]
    RestoreAutoDNS,
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

/// How disruptive a repair is; recommendations are ordered safest first.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum RepairRisk {
    /// No connectivity interruption (e.g. flushing a cache).
    Low,
    /// Brief interruption or a persistent but reversible setting change.
    Medium,
    /// Needs admin rights and may drop the connection or require a reboot.
    High,
}

/// Recommended repair action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairAction {
    pub action_type: RepairType,
    pub name: String,
    pub description: String,
    pub priority: u8, // 1-10, lower is higher priority
    pub estimated_time_seconds: u64,
    pub risk: RepairRisk,
}

/// Whether a quick-fix type is implemented on the running platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairCapability {
    pub fix_type: String,
    pub supported: bool,
    pub reason: Option<String>,
}

/// Complete diagnostic result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticResult {
    pub overall_status: DiagnosticStatus,
    pub network_connectivity: DiagnosticItem,
    pub ip_configuration: DiagnosticItem,
    pub dns_resolution: DiagnosticItem,
    pub network_quality: DiagnosticItem,
    /// Keys of the checks whose status is not `pass`
    /// (`network_connectivity`, `ip_configuration`, `dns_resolution`,
    /// `network_quality`). This, not `recommendations.len()`, is the number
    /// of problems found.
    pub problem_checks: Vec<String>,
    pub recommendations: Vec<RepairAction>,
    /// Guidance that is not an automated action (e.g. check the cable).
    pub hints: Vec<String>,
    pub timestamp: i64,
}
