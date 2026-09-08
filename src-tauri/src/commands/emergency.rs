use crate::models::{DiagnosticItem, DiagnosticResult, DiagnosticStatus, RepairAction, RepairType};
use tokio::time::{timeout, Duration};

/// Apply a quick fix action
/// Accepts snake_case format matching RepairType serialization
///
/// Runs the (potentially long-running, privilege-prompting) platform repair
/// on a blocking thread with a generous timeout — the macOS "reset adapter"
/// path waits on an osascript admin authorization dialog which could
/// otherwise stall the async runtime indefinitely.
#[tauri::command]
pub async fn apply_quick_fix(fix_type: String) -> Result<bool, String> {
    let fix_type_for_task = fix_type.clone();
    tokio::task::spawn_blocking(move || apply_quick_fix_blocking(&fix_type_for_task))
        .await
        .map_err(|e| format!("Fix task error: {}", e))?
}

fn apply_quick_fix_blocking(fix_type: &str) -> Result<bool, String> {
    match fix_type {
        "flush_dns_cache" => {
            tracing::info!("Executing fix: flush_dns_cache");
            crate::platform::flush_dns_cache().map_err(|e| e.to_string())?;
            Ok(true)
        }
        "release_renew_ip" => {
            tracing::info!("Executing fix: release_renew_ip");
            crate::platform::release_renew_ip().map_err(|e| e.to_string())?;
            Ok(true)
        }
        "reset_network_stack" => {
            tracing::info!("Executing fix: reset_network_stack");
            crate::platform::reset_network_stack().map_err(|e| e.to_string())?;
            Ok(true)
        }
        "switch_dns" => {
            tracing::info!("Executing fix: switch_dns");
            // Use the DNS servers configured in Settings (falling back to
            // 8.8.8.8 / 1.1.1.1 if settings can't be read). This links the
            // Settings DNS fields to the EmergencyKit "切换DNS" action.
            let (primary, secondary) = crate::commands::settings::load_settings_from_file()
                .map(|s| {
                    let sec = if s.secondary_dns.is_empty() {
                        None
                    } else {
                        Some(s.secondary_dns.clone())
                    };
                    (s.primary_dns, sec)
                })
                .unwrap_or_else(|_| ("8.8.8.8".to_string(), Some("1.1.1.1".to_string())));
            crate::platform::set_dns_servers(&primary, secondary.as_deref())
                .map_err(|e| format!("Failed to switch DNS: {}", e))?;
            Ok(true)
        }
        "toggle_ipv6" => {
            tracing::info!("Executing fix: toggle_ipv6");
            let desc = crate::platform::toggle_ipv6()
                .map_err(|e| format!("切换 IPv6 失败: {}", e))?;
            tracing::info!("toggle_ipv6 result: {}", desc);
            Ok(true)
        }
        "reset_adapter" => {
            tracing::info!("Executing fix: reset_adapter");
            crate::platform::reset_adapter().map_err(|e| format!("重置网络适配器失败: {}", e))?;
            Ok(true)
        }
        "restart_network_service" => {
            tracing::info!("Executing fix: restart_network_service");
            crate::platform::reset_network_stack()
                .map_err(|e| format!("重启网络服务失败: {}", e))?;
            Ok(true)
        }
        _ => {
            tracing::warn!("Unknown fix type: {}", fix_type);
            Err(format!("未知的修复类型: {}", fix_type))
        }
    }
}

/// Run network diagnostics
#[tauri::command]
pub async fn run_diagnostics() -> Result<DiagnosticResult, String> {
    tracing::info!("Starting network diagnostics...");

    // Run all diagnostic checks
    let network_connectivity = check_network_connectivity().await;
    let ip_configuration = check_ip_configuration().await;
    let dns_resolution = check_dns_resolution().await;
    let network_quality = check_network_quality().await;

    // Determine overall status
    let overall = if [
        &network_connectivity,
        &ip_configuration,
        &dns_resolution,
        &network_quality,
    ]
    .iter()
    .all(|d| d.status == DiagnosticStatus::Pass)
    {
        DiagnosticStatus::Pass
    } else if [
        &network_connectivity,
        &ip_configuration,
        &dns_resolution,
        &network_quality,
    ]
    .iter()
    .any(|d| d.status == DiagnosticStatus::Fail)
    {
        DiagnosticStatus::Fail
    } else {
        DiagnosticStatus::Warning
    };

    // Generate recommendations based on failures
    let recommendations = generate_recommendations(
        &network_connectivity,
        &ip_configuration,
        &dns_resolution,
        &network_quality,
    );

    tracing::info!(
        "Diagnostics completed: overall_status={:?}, {} recommendations",
        overall,
        recommendations.len()
    );

    Ok(DiagnosticResult {
        overall_status: overall,
        network_connectivity,
        ip_configuration,
        dns_resolution,
        network_quality,
        recommendations,
        timestamp: chrono::Utc::now().timestamp_millis(),
    })
}

/// Check network connectivity: first the local gateway, then the internet.
///
/// The old check only pinged 8.8.8.8 and labeled a failure "无法连接到网关"
/// even when the LAN itself was fine (and many ISPs block ICMP to public
/// servers, producing false failures). Now we probe the actual gateway
/// first and only then check the internet, with distinct messages.
async fn check_network_connectivity() -> DiagnosticItem {
    let start = std::time::Instant::now();
    let gateway = crate::platform::get_default_gateway().ok().flatten();

    // Gateway reachability (probe the real default gateway).
    let gateway_ok = match gateway {
        Some(gw) => {
            let target = gw.to_string();
            timeout(
                Duration::from_secs(4),
                tokio::task::spawn_blocking(move || ping_one(&target)),
            )
            .await
            .map(|r| r.unwrap_or(false))
            .unwrap_or(false)
        }
        None => false,
    };

    if !gateway_ok {
        let gw_label = gateway
            .map(|g| g.to_string())
            .unwrap_or_else(|| "未检测到网关".to_string());
        return DiagnosticItem {
            status: DiagnosticStatus::Fail,
            message: format!("无法连接到网关 {}", gw_label),
            details: serde_json::json!({ "gateway_reachable": false }),
            duration_ms: start.elapsed().as_millis() as u64,
        };
    }

    // Gateway reachable → probe the internet.
    let internet_ok = timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(|| ping_one("8.8.8.8")),
    )
    .await
    .map(|r| r.unwrap_or(false))
    .unwrap_or(false);

    if internet_ok {
        DiagnosticItem {
            status: DiagnosticStatus::Pass,
            message: "网络连接正常".to_string(),
            details: serde_json::json!({ "gateway_reachable": true, "internet_reachable": true }),
            duration_ms: start.elapsed().as_millis() as u64,
        }
    } else {
        // LAN works but the public probe failed — ICMP to 8.8.8.8 is often
        // blocked by ISPs, so do not label this a hard failure.
        DiagnosticItem {
            status: DiagnosticStatus::Warning,
            message: "网关可达，但无法访问公网".to_string(),
            details: serde_json::json!({ "gateway_reachable": true, "internet_reachable": false }),
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }
}

/// Ping one destination on the blocking thread pool and report success
/// (exit code 0, locale-independent).
fn ping_one(target: &str) -> bool {
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("ping")
        .args(["-n", "1", "-w", "3000", target])
        .output();

    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("ping")
        .args(["-c", "1", "-W", "3", target])
        .output();

    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("ping")
        .args(["-c", "1", "-W", "3000", target])
        .output();

    matches!(result, Ok(output) if output.status.success())
}

/// Check IP configuration
async fn check_ip_configuration() -> DiagnosticItem {
    let start = std::time::Instant::now();

    // Local-only probe: never performs external HTTP requests (public IP
    // fetch or GeoIP), so it is fast and offline-safe.
    let ip_info_result = timeout(
        Duration::from_secs(5),
        crate::commands::ip_info::get_local_ip_info_only(),
    )
    .await;

    match ip_info_result {
        Ok(Ok(info)) => {
            // A valid IP configuration means we have a usable LOCAL address
            // (the public `ipv4`/`ipv6` fields are None when there is no
            // internet, even though the LAN works — using them caused
            // false "未配置有效的IP地址" failures).
            let has_local_ipv4 = info.local_ipv4.is_some();
            let has_local_ipv6 = info.local_ipv6.is_some();
            if has_local_ipv4 || has_local_ipv6 {
                DiagnosticItem {
                    status: DiagnosticStatus::Pass,
                    message: "IP地址配置正常".to_string(),
                    details: serde_json::json!({
                        "local_ipv4": info.local_ipv4,
                        "local_ipv6": info.local_ipv6,
                        "dual_stack": info.dual_stack_enabled
                    }),
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            } else {
                DiagnosticItem {
                    status: DiagnosticStatus::Fail,
                    message: "未检测到有效的本地IP地址".to_string(),
                    details: serde_json::json!({}),
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
        }
        Ok(Err(e)) => {
            tracing::warn!("IP info check failed: {}", e);
            DiagnosticItem {
                status: DiagnosticStatus::Warning,
                message: "IP信息检测异常".to_string(),
                details: serde_json::json!({ "error": e }),
                duration_ms: start.elapsed().as_millis() as u64,
            }
        }
        Err(_) => {
            tracing::warn!("IP info check timed out after 10 seconds");
            DiagnosticItem {
                status: DiagnosticStatus::Warning,
                message: "IP信息检测超时".to_string(),
                details: serde_json::json!({ "timeout": true }),
                duration_ms: start.elapsed().as_millis() as u64,
            }
        }
    }
}

/// Check DNS resolution
async fn check_dns_resolution() -> DiagnosticItem {
    let start = std::time::Instant::now();

    match crate::commands::dns::test_dns("8.8.8.8".to_string()).await {
        Ok(stats) => {
            if stats.avg_latency_ms > 0.0 && stats.success_rate > 0.5 {
                DiagnosticItem {
                    status: DiagnosticStatus::Pass,
                    message: format!("DNS解析正常 ({}ms)", stats.avg_latency_ms.round()),
                    details: serde_json::json!({
                        "latency_ms": stats.avg_latency_ms,
                        "success_rate": stats.success_rate
                    }),
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            } else {
                DiagnosticItem {
                    status: DiagnosticStatus::Fail,
                    message: "DNS解析失败".to_string(),
                    details: serde_json::json!({}),
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
        }
        Err(_) => DiagnosticItem {
            status: DiagnosticStatus::Fail,
            message: "DNS测试失败".to_string(),
            details: serde_json::json!({}),
            duration_ms: start.elapsed().as_millis() as u64,
        },
    }
}

/// Check network quality
async fn check_network_quality() -> DiagnosticItem {
    let start = std::time::Instant::now();

    match crate::commands::network_quality::ping("8.8.8.8".to_string(), false).await {
        Ok(ping_result) => {
            if ping_result.success {
                let quality =
                    if ping_result.avg_latency_ms < 50.0 && ping_result.packet_loss_percent < 1.0 {
                        DiagnosticStatus::Pass
                    } else if ping_result.avg_latency_ms < 200.0
                        && ping_result.packet_loss_percent < 5.0
                    {
                        DiagnosticStatus::Warning
                    } else {
                        DiagnosticStatus::Fail
                    };

                DiagnosticItem {
                    status: quality,
                    message: format!(
                        "网络质量: {:.1}ms, 丢包{:.1}%",
                        ping_result.avg_latency_ms, ping_result.packet_loss_percent
                    ),
                    details: serde_json::json!({
                        "latency_ms": ping_result.avg_latency_ms,
                        "packet_loss_percent": ping_result.packet_loss_percent
                    }),
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            } else {
                DiagnosticItem {
                    status: DiagnosticStatus::Fail,
                    message: "网络质量测试失败".to_string(),
                    details: serde_json::json!({}),
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
        }
        Err(_) => DiagnosticItem {
            status: DiagnosticStatus::Fail,
            message: "网络质量测试异常".to_string(),
            details: serde_json::json!({}),
            duration_ms: start.elapsed().as_millis() as u64,
        },
    }
}

/// Platform-specific description of what "reset network stack" actually does.
fn reset_network_stack_description() -> String {
    #[cfg(target_os = "windows")]
    {
        "重置 Winsock 与 TCP/IP 协议栈（需要管理员权限，通常需要重启电脑，会清空部分网络配置）"
            .to_string()
    }
    #[cfg(target_os = "linux")]
    {
        "重启 NetworkManager 网络服务（需要 root 权限）".to_string()
    }
    #[cfg(target_os = "macos")]
    {
        "重启 mDNSResponder 解析服务并清空 DNS 缓存（需要管理员权限）".to_string()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        "重置网络协议栈".to_string()
    }
}

/// Generate repair recommendations
fn generate_recommendations(
    network_connectivity: &DiagnosticItem,
    ip_configuration: &DiagnosticItem,
    dns_resolution: &DiagnosticItem,
    network_quality: &DiagnosticItem,
) -> Vec<RepairAction> {
    let mut recommendations = Vec::new();

    if dns_resolution.status != DiagnosticStatus::Pass {
        recommendations.push(RepairAction {
            action_type: RepairType::SwitchDNS,
            name: "切换DNS服务器".to_string(),
            description: "切换到设置中配置的备用 DNS 服务器".to_string(),
            priority: 1,
            estimated_time_seconds: 5,
        });
        recommendations.push(RepairAction {
            action_type: RepairType::FlushDNSCache,
            name: "清空DNS缓存".to_string(),
            description: "清空本地DNS解析缓存".to_string(),
            priority: 2,
            estimated_time_seconds: 2,
        });
    }

    if ip_configuration.status != DiagnosticStatus::Pass {
        recommendations.push(RepairAction {
            action_type: RepairType::ReleaseRenewIP,
            name: "重新获取IP地址".to_string(),
            description: "释放当前IP并重新向DHCP服务器请求".to_string(),
            priority: 1,
            estimated_time_seconds: 10,
        });
        recommendations.push(RepairAction {
            action_type: RepairType::ResetAdapter,
            name: "重置网络适配器".to_string(),
            description: "禁用并重新启用主网络适配器（需管理员授权）".to_string(),
            priority: 2,
            estimated_time_seconds: 15,
        });
    }

    if network_quality.status != DiagnosticStatus::Pass
        || network_connectivity.status != DiagnosticStatus::Pass
    {
        // One stack-reset recommendation (the old code emitted two options
        // with different names that both ran the same reset_network_stack).
        recommendations.push(RepairAction {
            action_type: RepairType::ResetNetworkStack,
            name: "重置网络协议栈".to_string(),
            description: reset_network_stack_description(),
            priority: 1,
            estimated_time_seconds: 30,
        });
    }

    // Sort by priority
    recommendations.sort_by_key(|r| r.priority);

    // Limit to top 3 recommendations
    recommendations.truncate(3);

    recommendations
}
