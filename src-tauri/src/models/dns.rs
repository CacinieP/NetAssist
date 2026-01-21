#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// DNS statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DNSStats {
    /// DNS server address
    pub server: String,
    /// Average response time in milliseconds
    pub avg_latency_ms: f64,
    /// Query success rate (0.0 to 1.0)
    pub success_rate: f64,
    /// Total queries
    pub total_queries: u64,
    /// Failed queries
    pub failed_queries: u64,
    /// Cache hit rate (0.0 to 1.0)
    pub cache_hit_rate: f64,
}

/// DNS record information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DNSRecord {
    pub name: String,
    pub record_type: String, // "A" or "AAAA"
    pub value: String,
    pub ttl: u32,
}
