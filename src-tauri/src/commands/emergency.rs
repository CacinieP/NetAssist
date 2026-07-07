use crate::models::{DiagnosticItem, DiagnosticResult, DiagnosticStatus, RepairAction, RepairType};
use tokio::time::{timeout, Duration};

/// Apply a quick fix action
/// Accepts snake_case format matching RepairType serialization
#[tauri::command]
pub async fn apply_quick_fix(fix_type: String) -> Result<bool, String> {
    match fix_type.as_str() {
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
            crate::platform::toggle_ipv6().map_err(|e| format!("切换 IPv6 失败: {}", e))?;
            Ok(true)
        }
        "reset_adapter" => {
            tracing::info!("Executing fix: reset_adapter");
            crate::platform::reset_adapter().map_err(|e| format!("重置网络适配器失败: {}", e))?;
            Ok(true)
        }
        "restart_network_service" => {
            tracing::info!("Executing fix: restart_network_service");
            // Restart network service - reset network stack
            crate::platform::reset_network_stack()
                .map_err(|e| format!("Failed to restart network service: {}", e))?;
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

/// Check network connectivity
async fn check_network_connectivity() -> DiagnosticItem {
    let start = std::time::Instant::now();

    // Try to ping a reliable server with timeout (5 seconds)
    let ping_result = timeout(
        Duration::from_secs(5),
        tokio::task::spawn_blocking(|| {
            #[cfg(target_os = "windows")]
            let result = std::process::Command::new("ping")
                .args(&["-n", "1", "-w", "3000", "8.8.8.8"])
                .output();

            #[cfg(target_os = "linux")]
            let result = std::process::Command::new("ping")
                .args(&["-c", "1", "-W", "3", "8.8.8.8"])
                .output();

            #[cfg(target_os = "macos")]
            let result = std::process::Command::new("ping")
                .args(["-c", "1", "-W", "3000", "8.8.8.8"])
                .output();

            result
        }),
    )
    .await;

    match ping_result {
        Ok(join_result) => match join_result {
            Ok(output_result) => match output_result {
                Ok(output) => {
                    // Prefer the ping process exit code over parsing its
                    // (locale-dependent) text output: ping exits 0 on success
                    // regardless of system language, which is more reliable
                    // than matching "TTL="/"bytes from" strings.
                    let success = output.status.success();

                    if success {
                        DiagnosticItem {
                            status: DiagnosticStatus::Pass,
                            message: "网络连接正常".to_string(),
                            details: serde_json::json!({ "gateway_reachable": true }),
                            duration_ms: start.elapsed().as_millis() as u64,
                        }
                    } else {
                        DiagnosticItem {
                            status: DiagnosticStatus::Fail,
                            message: "无法连接到网关".to_string(),
                            details: serde_json::json!({}),
                            duration_ms: start.elapsed().as_millis() as u64,
                        }
                    }
                }
                Err(_) => {
                    tracing::warn!("Ping command execution failed");
                    DiagnosticItem {
                        status: DiagnosticStatus::Fail,
                        message: "Ping命令执行失败".to_string(),
                        details: serde_json::json!({}),
                        duration_ms: start.elapsed().as_millis() as u64,
                    }
                }
            },
            Err(_) => {
                tracing::warn!("Ping task join failed");
                DiagnosticItem {
                    status: DiagnosticStatus::Fail,
                    message: "Ping任务失败".to_string(),
                    details: serde_json::json!({}),
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            }
        },
        Err(_) => {
            tracing::warn!("Network connectivity check timed out after 5 seconds");
            DiagnosticItem {
                status: DiagnosticStatus::Fail,
                message: "网络诊断超时".to_string(),
                details: serde_json::json!({ "timeout": true }),
                duration_ms: start.elapsed().as_millis() as u64,
            }
        }
    }
}

/// Check IP configuration
async fn check_ip_configuration() -> DiagnosticItem {
    let start = std::time::Instant::now();

    // Add timeout to IP info check (20 seconds - needs to fetch public IP)
    let ip_info_result = timeout(
        Duration::from_secs(20),
        crate::commands::ip_info::get_ip_info(None),
    )
    .await;

    match ip_info_result {
        Ok(Ok(info)) => {
            let has_valid_ip = info.ipv4.is_some() || info.ipv6.is_some();
            if has_valid_ip {
                DiagnosticItem {
                    status: DiagnosticStatus::Pass,
                    message: "IP地址配置正常".to_string(),
                    details: serde_json::json!({
                        "ipv4": info.ipv4,
                        "ipv6": info.ipv6,
                        "dual_stack": info.dual_stack_enabled
                    }),
                    duration_ms: start.elapsed().as_millis() as u64,
                }
            } else {
                DiagnosticItem {
                    status: DiagnosticStatus::Fail,
                    message: "未配置有效的IP地址".to_string(),
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
            tracing::warn!("IP info check timed out after 20 seconds");
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
            description: "切换到备用DNS服务器（8.8.8.8 或 1.1.1.1）".to_string(),
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

    if network_quality.status != DiagnosticStatus::Pass {
        recommendations.push(RepairAction {
            action_type: RepairType::ResetNetworkStack,
            name: "刷新DNS解析服务".to_string(),
            description: "重启mDNSResponder解析服务并清空DNS缓存".to_string(),
            priority: 1,
            estimated_time_seconds: 30,
        });
    }

    if network_connectivity.status != DiagnosticStatus::Pass {
        recommendations.push(RepairAction {
            action_type: RepairType::RestartNetworkService,
            name: "刷新网络解析服务".to_string(),
            description: "重启DNS解析服务以重置网络通信".to_string(),
            priority: 1,
            estimated_time_seconds: 20,
        });
    }

    // Sort by priority
    recommendations.sort_by_key(|r| r.priority);

    // Limit to top 3 recommendations
    recommendations.truncate(3);

    recommendations
}
