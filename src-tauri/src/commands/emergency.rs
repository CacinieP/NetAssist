use crate::models::{
    DiagnosticItem, DiagnosticResult, DiagnosticStatus, RepairAction, RepairCapability, RepairRisk,
    RepairType,
};
use std::net::{IpAddr, SocketAddr};
use tauri::Emitter;
use tokio::time::{timeout, Duration};

/// Upper bound for one repair, including time spent on an admin
/// authorization dialog.
const FIX_TIMEOUT: Duration = Duration::from_secs(300);

/// Apply a quick fix action
/// Accepts snake_case format matching RepairType serialization
///
/// Runs the (potentially long-running, privilege-prompting) platform repair
/// on a blocking thread with a generous timeout — the macOS "reset adapter"
/// path waits on an osascript admin authorization dialog which could
/// otherwise stall the async runtime indefinitely.
///
/// The timeout only releases the caller: the blocking thread (and any
/// authorization dialog it spawned) cannot be cancelled and may still
/// complete later.
#[tauri::command]
pub async fn apply_quick_fix(fix_type: String) -> Result<bool, String> {
    let fix_type_for_task = fix_type.clone();
    let task = tokio::task::spawn_blocking(move || apply_quick_fix_blocking(&fix_type_for_task));
    match timeout(FIX_TIMEOUT, task).await {
        Ok(joined) => joined.map_err(|e| format!("Fix task error: {}", e))?,
        Err(_) => {
            tracing::warn!("Fix {} timed out after {:?}", fix_type, FIX_TIMEOUT);
            Err(format!(
                "修复操作超时（{} 分钟）未完成；如果系统授权框仍未关闭，之后确认授权该操作仍可能执行",
                FIX_TIMEOUT.as_secs() / 60
            ))
        }
    }
}

fn apply_quick_fix_blocking(fix_type: &str) -> Result<bool, String> {
    if let Some(reason) = unsupported_reason(fix_type) {
        return Err(reason.to_string());
    }
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
            let before = crate::platform::get_dns_servers().unwrap_or_default();
            tracing::info!(
                "switch_dns: {:?} -> {} {:?} (undo with restore_auto_dns)",
                before,
                primary,
                secondary
            );
            crate::platform::set_dns_servers(&primary, secondary.as_deref())
                .map_err(|e| format!("切换 DNS 失败: {}", e))?;
            Ok(true)
        }
        "restore_auto_dns" => {
            tracing::info!("Executing fix: restore_auto_dns");
            crate::platform::restore_auto_dns().map_err(|e| format!("恢复自动 DNS 失败: {}", e))?;
            Ok(true)
        }
        "toggle_ipv6" => {
            tracing::info!("Executing fix: toggle_ipv6");
            let desc =
                crate::platform::toggle_ipv6().map_err(|e| format!("切换 IPv6 失败: {}", e))?;
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

/// Fix types offered in the UI, in display order. `restart_network_service`
/// is still accepted by `apply_quick_fix` for older callers, but it runs the
/// same action as `reset_network_stack`, so it is not listed.
const REPAIR_TYPES: &[&str] = &[
    "flush_dns_cache",
    "switch_dns",
    "restore_auto_dns",
    "release_renew_ip",
    "toggle_ipv6",
    "reset_adapter",
    "reset_network_stack",
];

/// Why a fix type cannot run on this platform, if it cannot.
fn unsupported_reason(fix_type: &str) -> Option<&'static str> {
    match fix_type {
        "toggle_ipv6" | "reset_adapter" if !cfg!(target_os = "macos") => {
            Some("当前平台暂不支持（目前仅 macOS 已实现）")
        }
        _ => None,
    }
}

/// Report which quick fixes the running platform implements, so the UI can
/// disable the rest instead of letting them fail.
#[tauri::command]
pub fn get_repair_capabilities() -> Vec<RepairCapability> {
    REPAIR_TYPES
        .iter()
        .map(|fix_type| {
            let reason = unsupported_reason(fix_type);
            RepairCapability {
                fix_type: fix_type.to_string(),
                supported: reason.is_none(),
                reason: reason.map(str::to_string),
            }
        })
        .collect()
}

/// Event emitted as each diagnostic check finishes.
pub const DIAGNOSTICS_PROGRESS_EVENT: &str = "diagnostics-progress";

/// Per-check bound; the checks run concurrently, so this is also roughly the
/// bound for the whole diagnosis.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, serde::Serialize)]
struct DiagnosticsProgress {
    run_id: Option<u32>,
    check: &'static str,
    status: DiagnosticStatus,
    message: String,
}

async fn run_check(
    app: &tauri::AppHandle,
    run_id: Option<u32>,
    key: &'static str,
    check: impl std::future::Future<Output = DiagnosticItem>,
) -> DiagnosticItem {
    let start = std::time::Instant::now();
    let item = timeout(CHECK_TIMEOUT, check)
        .await
        .unwrap_or_else(|_| DiagnosticItem {
            status: DiagnosticStatus::Warning,
            message: format!("检查超时（{} 秒），结果未知", CHECK_TIMEOUT.as_secs()),
            details: serde_json::json!({ "timeout": true }),
            duration_ms: start.elapsed().as_millis() as u64,
        });
    let progress = DiagnosticsProgress {
        run_id,
        check: key,
        status: item.status.clone(),
        message: item.message.clone(),
    };
    if let Err(e) = app.emit(DIAGNOSTICS_PROGRESS_EVENT, progress) {
        tracing::debug!("Failed to emit diagnostics progress: {}", e);
    }
    item
}

/// Run network diagnostics
///
/// The four checks are independent and run concurrently; each emits a
/// `diagnostics-progress` event when it finishes and is bounded by
/// `CHECK_TIMEOUT`, so a stuck probe yields a partial result instead of an
/// endless wait. `run_id` is echoed in every progress event so the caller
/// can ignore events from a diagnosis it has already abandoned.
#[tauri::command]
pub async fn run_diagnostics(
    app: tauri::AppHandle,
    run_id: Option<u32>,
) -> Result<DiagnosticResult, String> {
    tracing::info!("Starting network diagnostics...");

    let (network_connectivity, ip_configuration, dns_resolution, network_quality) = tokio::join!(
        run_check(
            &app,
            run_id,
            "network_connectivity",
            check_network_connectivity()
        ),
        run_check(&app, run_id, "ip_configuration", check_ip_configuration()),
        run_check(&app, run_id, "dns_resolution", check_dns_resolution()),
        run_check(&app, run_id, "network_quality", check_network_quality()),
    );

    let checks = [
        ("network_connectivity", &network_connectivity),
        ("ip_configuration", &ip_configuration),
        ("dns_resolution", &dns_resolution),
        ("network_quality", &network_quality),
    ];
    let overall = if checks
        .iter()
        .all(|(_, d)| d.status == DiagnosticStatus::Pass)
    {
        DiagnosticStatus::Pass
    } else if checks
        .iter()
        .any(|(_, d)| d.status == DiagnosticStatus::Fail)
    {
        DiagnosticStatus::Fail
    } else {
        DiagnosticStatus::Warning
    };
    let problem_checks: Vec<String> = checks
        .iter()
        .filter(|(_, d)| d.status != DiagnosticStatus::Pass)
        .map(|(key, _)| key.to_string())
        .collect();

    let (recommendations, hints) = generate_recommendations(
        &network_connectivity,
        &ip_configuration,
        &dns_resolution,
        &network_quality,
    );

    tracing::info!(
        "Diagnostics completed: overall_status={:?}, {} problems, {} recommendations",
        overall,
        problem_checks.len(),
        recommendations.len()
    );

    Ok(DiagnosticResult {
        overall_status: overall,
        network_connectivity,
        ip_configuration,
        dns_resolution,
        network_quality,
        problem_checks,
        recommendations,
        hints,
        timestamp: chrono::Utc::now().timestamp_millis(),
    })
}

/// Public HTTP endpoints for a reachability check when ICMP gets no answer.
/// Several operators are listed because any single one may be blocked.
const PUBLIC_HTTP_TARGETS: [&str; 3] = ["1.1.1.1:80", "223.5.5.5:80", "1.0.0.1:80"];

fn public_http_targets() -> Vec<SocketAddr> {
    PUBLIC_HTTP_TARGETS
        .iter()
        .filter_map(|t| t.parse().ok())
        .collect()
}

/// How a reachability probe decides the far end answered.
#[derive(Clone, Copy)]
enum Probe {
    /// A completed or refused TCP handshake (the RST proves the host is
    /// there). Used for the LAN gateway.
    Handshake,
    /// An HTTP response to a HEAD request. Needed for public hosts because
    /// TUN-mode proxies accept every TCP handshake locally, so a handshake
    /// alone does not prove the internet is reachable.
    HttpResponse,
}

/// Latency until `addr` answered according to `probe`.
async fn tcp_reachable(addr: SocketAddr, wait: Duration, probe: Probe) -> Option<f64> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let start = std::time::Instant::now();
    let elapsed_ms = || start.elapsed().as_secs_f64() * 1000.0;
    let attempt = async {
        match tokio::net::TcpStream::connect(addr).await {
            Ok(mut stream) => match probe {
                Probe::Handshake => Some(elapsed_ms()),
                Probe::HttpResponse => {
                    let request = format!(
                        "HEAD / HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                        addr.ip()
                    );
                    stream.write_all(request.as_bytes()).await.ok()?;
                    let mut buf = [0u8; 16];
                    match stream.read(&mut buf).await {
                        Ok(n) if n > 0 => Some(elapsed_ms()),
                        _ => None,
                    }
                }
            },
            Err(e)
                if matches!(probe, Probe::Handshake)
                    && e.kind() == std::io::ErrorKind::ConnectionRefused =>
            {
                Some(elapsed_ms())
            }
            Err(_) => None,
        }
    };
    timeout(wait, attempt).await.ok().flatten()
}

/// First of `targets` that answers `probe`, probing all of them at once.
async fn first_tcp_reachable(
    targets: Vec<SocketAddr>,
    wait: Duration,
    probe: Probe,
) -> Option<(SocketAddr, f64)> {
    let mut probes = tokio::task::JoinSet::new();
    for addr in targets {
        probes.spawn(async move { tcp_reachable(addr, wait, probe).await.map(|ms| (addr, ms)) });
    }
    while let Some(joined) = probes.join_next().await {
        if let Ok(Some(hit)) = joined {
            return Some(hit);
        }
    }
    None
}

/// TCP ports most home/office gateways answer on (web UI, DNS).
fn gateway_tcp_targets(gw: &crate::platform::DefaultGateway) -> Vec<SocketAddr> {
    let base = match (gw.addr, gw.zone.as_deref()) {
        (IpAddr::V4(v4), _) => SocketAddr::from((v4, 0)),
        (IpAddr::V6(v6), None) => SocketAddr::from((v6, 0)),
        // Only numeric zones (Windows interface indexes) can be expressed
        // as a socket scope id without a name lookup.
        (IpAddr::V6(v6), Some(zone)) => match zone.parse::<u32>() {
            Ok(scope) => SocketAddr::V6(std::net::SocketAddrV6::new(v6, 0, 0, scope)),
            Err(_) => return Vec::new(),
        },
    };
    [80, 443, 53]
        .iter()
        .map(|port| {
            let mut addr = base;
            addr.set_port(*port);
            addr
        })
        .collect()
}

async fn ping_with_timeout(target: String, wait: Duration) -> bool {
    timeout(wait, tokio::task::spawn_blocking(move || ping_one(&target)))
        .await
        .map(|r| r.unwrap_or(false))
        .unwrap_or(false)
}

/// Check network connectivity: first the default-route gateway, then the
/// internet.
///
/// ICMP is tried first, with TCP fallbacks for both hops (a handshake for
/// the gateway, an HTTP response for the internet), because many networks filter ICMP; a host is only reported
/// unreachable when neither answers.
async fn check_network_connectivity() -> DiagnosticItem {
    let start = std::time::Instant::now();
    let gateway = tokio::task::spawn_blocking(crate::platform::get_default_gateway)
        .await
        .ok()
        .and_then(|r| r.ok())
        .flatten();

    let Some(gw) = gateway else {
        return DiagnosticItem {
            status: DiagnosticStatus::Fail,
            message: "未检测到默认网关（可能尚未连接到任何网络）".to_string(),
            details: serde_json::json!({ "gateway": null, "gateway_reachable": false }),
            duration_ms: start.elapsed().as_millis() as u64,
        };
    };
    let gw_label = gw.ping_target();

    let gateway_probe = if ping_with_timeout(gw_label.clone(), Duration::from_secs(4)).await {
        Some("icmp")
    } else if first_tcp_reachable(
        gateway_tcp_targets(&gw),
        Duration::from_millis(1500),
        Probe::Handshake,
    )
    .await
    .is_some()
    {
        Some("tcp")
    } else {
        None
    };

    if gateway_probe.is_none() {
        return DiagnosticItem {
            status: DiagnosticStatus::Fail,
            message: format!("无法连接到网关 {}（ICMP 与 TCP 均无响应）", gw_label),
            details: serde_json::json!({
                "gateway": gw_label,
                "interface": gw.interface,
                "gateway_reachable": false,
            }),
            duration_ms: start.elapsed().as_millis() as u64,
        };
    }

    let internet = if ping_with_timeout("8.8.8.8".to_string(), Duration::from_secs(5)).await {
        Some(("icmp", "8.8.8.8".to_string()))
    } else {
        first_tcp_reachable(
            public_http_targets(),
            Duration::from_secs(3),
            Probe::HttpResponse,
        )
        .await
        .map(|(addr, _)| ("http", addr.to_string()))
    };

    let details = serde_json::json!({
        "gateway": gw_label,
        "interface": gw.interface,
        "gateway_reachable": true,
        "gateway_probe": gateway_probe,
        "internet_reachable": internet.is_some(),
        "internet_probe": internet.as_ref().map(|(method, _)| *method),
        "internet_target": internet.as_ref().map(|(_, target)| target.clone()),
    });
    let (status, message) = match &internet {
        Some(("icmp", _)) => (DiagnosticStatus::Pass, "网络连接正常".to_string()),
        Some((_, target)) => (
            DiagnosticStatus::Pass,
            format!("网络连接正常（ICMP 被过滤，已通过 HTTP {} 确认）", target),
        ),
        None => (
            DiagnosticStatus::Warning,
            "网关可达，但无法直连公网（ICMP 与 HTTP 探测均无响应；如通过代理上网可忽略）"
                .to_string(),
        ),
    };
    DiagnosticItem {
        status,
        message,
        details,
        duration_ms: start.elapsed().as_millis() as u64,
    }
}

/// Ping one destination on the blocking thread pool and report success
/// (exit code 0, locale-independent). IPv6 targets may carry a `%zone`.
fn ping_one(target: &str) -> bool {
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("ping")
        .args(["-n", "1", "-w", "3000", target])
        .output();

    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("ping")
        .args(["-c", "1", "-W", "3", target])
        .output();

    // macOS `ping` is IPv4-only (it resolves "::1" as a hostname), so IPv6
    // targets need `ping6`.
    #[cfg(target_os = "macos")]
    let result = if target.contains(':') {
        std::process::Command::new("ping6")
            .args(["-c", "1", target])
            .output()
    } else {
        std::process::Command::new("ping")
            .args(["-c", "1", "-W", "3000", target])
            .output()
    };

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

/// Public resolver used when no system DNS server can be read, and as a
/// control probe next to the system resolver.
const CONTROL_DNS: &str = "8.8.8.8";

/// First system DNS server that `test_dns` can probe directly (an IP
/// literal, optionally bracketed). Scoped IPv6 entries such as
/// `fe80::1%en0` are skipped because the zone cannot be carried into the
/// probe socket address.
fn pick_system_dns(servers: &[String]) -> Option<String> {
    servers
        .iter()
        .map(|s| s.trim().trim_start_matches('[').trim_end_matches(']'))
        .find(|s| {
            s.parse::<std::net::IpAddr>()
                .map(|ip| !ip.is_unspecified())
                .unwrap_or(false)
        })
        .map(str::to_string)
}

/// `Some(latency_ms)` when the server answered most queries.
async fn probe_dns(server: &str) -> Option<f64> {
    match crate::commands::dns::test_dns(server.to_string()).await {
        Ok(stats) if stats.avg_latency_ms > 0.0 && stats.success_rate > 0.5 => {
            Some(stats.avg_latency_ms)
        }
        _ => None,
    }
}

/// Check DNS resolution against the resolver the system is actually
/// configured to use, with a public resolver as a control so a broken local
/// DNS configuration is not reported as healthy.
async fn check_dns_resolution() -> DiagnosticItem {
    let start = std::time::Instant::now();

    let system_servers = tokio::task::spawn_blocking(crate::platform::get_dns_servers)
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or_default();
    let system_dns = pick_system_dns(&system_servers);
    let target = system_dns
        .clone()
        .unwrap_or_else(|| CONTROL_DNS.to_string());
    let source = if system_dns.is_some() {
        "system"
    } else {
        "fallback"
    };

    let (target_latency, control_latency) = if target == CONTROL_DNS {
        let latency = probe_dns(&target).await;
        (latency, latency)
    } else {
        tokio::join!(probe_dns(&target), probe_dns(CONTROL_DNS))
    };

    let details = serde_json::json!({
        "server": target,
        "server_source": source,
        "system_servers": system_servers,
        "latency_ms": target_latency,
        "control_server": CONTROL_DNS,
        "control_latency_ms": control_latency,
    });
    let source_label = if system_dns.is_some() {
        "系统 DNS"
    } else {
        "未读取到系统 DNS，改测"
    };

    let (status, message) = match (target_latency, control_latency) {
        (Some(ms), _) => (
            DiagnosticStatus::Pass,
            format!(
                "DNS解析正常（{} {}，{}ms）",
                source_label,
                target,
                ms.round()
            ),
        ),
        (None, Some(_)) => (
            DiagnosticStatus::Fail,
            format!(
                "{} {} 解析失败，公共 DNS {} 可用",
                source_label, target, CONTROL_DNS
            ),
        ),
        (None, None) => (
            DiagnosticStatus::Fail,
            if target == CONTROL_DNS {
                format!("DNS解析失败（{} {}）", source_label, target)
            } else {
                format!(
                    "DNS解析失败（{} {} 与公共 DNS {} 均无响应）",
                    source_label, target, CONTROL_DNS
                )
            },
        ),
    };

    DiagnosticItem {
        status,
        message,
        details,
        duration_ms: start.elapsed().as_millis() as u64,
    }
}

/// Check network quality
///
/// Latency and loss are measured with ICMP. When ICMP gets no answer at
/// all, that is treated the same way as in the connectivity check: it is
/// often filtered, so the result is a warning with HTTP evidence, not a
/// failure.
async fn check_network_quality() -> DiagnosticItem {
    let start = std::time::Instant::now();

    if let Ok(ping_result) =
        crate::commands::network_quality::ping("8.8.8.8".to_string(), false).await
    {
        if ping_result.success {
            let quality = if ping_result.avg_latency_ms < 50.0
                && ping_result.packet_loss_percent < 1.0
            {
                DiagnosticStatus::Pass
            } else if ping_result.avg_latency_ms < 200.0 && ping_result.packet_loss_percent < 5.0 {
                DiagnosticStatus::Warning
            } else {
                DiagnosticStatus::Fail
            };

            return DiagnosticItem {
                status: quality,
                message: format!(
                    "网络质量: {:.1}ms, 丢包{:.1}%",
                    ping_result.avg_latency_ms, ping_result.packet_loss_percent
                ),
                details: serde_json::json!({
                    "method": "icmp",
                    "latency_ms": ping_result.avg_latency_ms,
                    "packet_loss_percent": ping_result.packet_loss_percent
                }),
                duration_ms: start.elapsed().as_millis() as u64,
            };
        }
    }

    let (message, details) = match first_tcp_reachable(
        public_http_targets(),
        Duration::from_secs(3),
        Probe::HttpResponse,
    )
    .await
    {
        Some((addr, ms)) => (
            format!(
                "ICMP 无响应（可能被过滤），无法测量丢包；HTTP {} 响应 {:.0}ms",
                addr, ms
            ),
            serde_json::json!({
                "method": "http",
                "icmp_reachable": false,
                "http_target": addr.to_string(),
                "http_latency_ms": ms
            }),
        ),
        None => (
            "ICMP 与 HTTP 探测均无响应，无法评估网络质量".to_string(),
            serde_json::json!({ "icmp_reachable": false, "http_reachable": false }),
        ),
    };
    DiagnosticItem {
        status: DiagnosticStatus::Warning,
        message,
        details,
        duration_ms: start.elapsed().as_millis() as u64,
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

fn repair_action(action_type: RepairType) -> RepairAction {
    let (name, description, priority, estimated_time_seconds, risk) = match action_type {
        RepairType::FlushDNSCache => (
            "清空DNS缓存",
            "清空本地DNS解析缓存".to_string(),
            1,
            2,
            RepairRisk::Low,
        ),
        RepairType::SwitchDNS => (
            "切换DNS服务器",
            "把主网络服务的 DNS 改为设置中配置的服务器（持久修改系统配置）".to_string(),
            2,
            5,
            RepairRisk::Medium,
        ),
        RepairType::RestoreAutoDNS => (
            "恢复自动DNS",
            "撤销切换DNS，改回由网络（DHCP）自动下发的 DNS 服务器".to_string(),
            2,
            5,
            RepairRisk::Medium,
        ),
        RepairType::ReleaseRenewIP => (
            "重新获取IP地址",
            "释放当前IP并重新向DHCP服务器请求，网络会短暂中断".to_string(),
            1,
            10,
            RepairRisk::Medium,
        ),
        RepairType::ResetAdapter => (
            "重置网络适配器",
            "禁用并重新启用主网络适配器（需管理员授权）".to_string(),
            2,
            15,
            RepairRisk::High,
        ),
        RepairType::ResetNetworkStack | RepairType::RestartNetworkService => (
            "重置网络协议栈",
            reset_network_stack_description(),
            3,
            30,
            RepairRisk::High,
        ),
        RepairType::ToggleIPv6 => (
            "切换IPv6",
            "在主网络服务上启用或关闭 IPv6".to_string(),
            3,
            5,
            RepairRisk::Medium,
        ),
        RepairType::DisconnectAbnormal => (
            "断开异常连接",
            "断开异常的网络连接".to_string(),
            3,
            5,
            RepairRisk::Medium,
        ),
    };
    RepairAction {
        action_type,
        name: name.to_string(),
        description,
        priority,
        estimated_time_seconds,
        risk,
    }
}

fn fix_type_name(action_type: &RepairType) -> String {
    serde_json::to_value(action_type)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Generate repair recommendations and non-automated hints.
///
/// Actions are de-duplicated, limited to what this platform supports, and
/// ordered safest first (risk, then priority) before keeping the top three,
/// so a disruptive action never pushes a harmless one out of the list.
fn generate_recommendations(
    network_connectivity: &DiagnosticItem,
    ip_configuration: &DiagnosticItem,
    dns_resolution: &DiagnosticItem,
    network_quality: &DiagnosticItem,
) -> (Vec<RepairAction>, Vec<String>) {
    let mut actions: Vec<RepairType> = Vec::new();
    let mut hints: Vec<String> = Vec::new();
    let gateway_unreachable = network_connectivity.status == DiagnosticStatus::Fail;

    if dns_resolution.status != DiagnosticStatus::Pass {
        actions.push(RepairType::FlushDNSCache);
        let control_ok = !dns_resolution.details["control_latency_ms"].is_null();
        let tested_system_dns = dns_resolution.details["server_source"] == "system";
        // Switching resolvers only helps when another resolver works.
        if control_ok && tested_system_dns {
            actions.push(RepairType::SwitchDNS);
        } else if !control_ok && !gateway_unreachable {
            hints.push("公共 DNS 也无响应，问题可能不在 DNS 本身，请先排查网络连接".to_string());
        }
    }

    if ip_configuration.status != DiagnosticStatus::Pass {
        actions.push(RepairType::ReleaseRenewIP);
        actions.push(RepairType::ResetAdapter);
    }

    if gateway_unreachable {
        hints.extend([
            "检查网线是否插好，或 Wi-Fi 是否已连接到正确的网络".to_string(),
            "确认路由器/光猫已通电且指示灯正常，必要时重启路由器".to_string(),
            "在系统网络设置中确认网络适配器已启用".to_string(),
        ]);
        actions.push(RepairType::ReleaseRenewIP);
        actions.push(RepairType::ResetNetworkStack);
    } else if network_connectivity.status == DiagnosticStatus::Warning {
        hints.push(
            "局域网正常但无法直连公网：可能是运营商或上游线路问题，可尝试重启路由器；通过代理上网时可忽略"
                .to_string(),
        );
    }

    if network_quality.status == DiagnosticStatus::Fail {
        hints.push("延迟或丢包偏高：可尝试靠近路由器、改用有线连接或稍后重试".to_string());
    }

    let mut seen: Vec<RepairType> = Vec::new();
    let mut recommendations: Vec<RepairAction> = actions
        .into_iter()
        .filter(|a| {
            if seen.contains(a) || unsupported_reason(&fix_type_name(a)).is_some() {
                return false;
            }
            seen.push(a.clone());
            true
        })
        .map(repair_action)
        .collect();
    recommendations.sort_by_key(|r| (r.risk, r.priority));
    recommendations.truncate(3);

    (recommendations, hints)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn servers(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn picks_first_usable_system_server() {
        assert_eq!(
            pick_system_dns(&servers(&["192.168.1.1", "8.8.8.8"])),
            Some("192.168.1.1".to_string())
        );
        assert_eq!(
            pick_system_dns(&servers(&["127.0.0.1"])),
            Some("127.0.0.1".to_string())
        );
    }

    #[test]
    fn skips_scoped_unspecified_and_hostname_entries() {
        assert_eq!(
            pick_system_dns(&servers(&[
                "fe80::1%en0",
                "0.0.0.0",
                "dns.local",
                "[2001:db8::53]"
            ])),
            Some("2001:db8::53".to_string())
        );
    }

    #[test]
    fn no_usable_server_returns_none() {
        assert_eq!(pick_system_dns(&[]), None);
        assert_eq!(pick_system_dns(&servers(&["fe80::1%en0", "  "])), None);
    }

    fn item(status: DiagnosticStatus, details: serde_json::Value) -> DiagnosticItem {
        DiagnosticItem {
            status,
            message: String::new(),
            details,
            duration_ms: 0,
        }
    }

    fn pass() -> DiagnosticItem {
        item(DiagnosticStatus::Pass, serde_json::json!({}))
    }

    fn types(actions: &[RepairAction]) -> Vec<RepairType> {
        actions.iter().map(|a| a.action_type.clone()).collect()
    }

    #[test]
    fn dns_failure_with_working_public_dns_offers_flush_then_switch() {
        let dns = item(
            DiagnosticStatus::Fail,
            serde_json::json!({ "server_source": "system", "control_latency_ms": 12.0 }),
        );
        let (actions, hints) = generate_recommendations(&pass(), &pass(), &dns, &pass());
        assert_eq!(
            types(&actions),
            vec![RepairType::FlushDNSCache, RepairType::SwitchDNS]
        );
        assert!(hints.is_empty());
    }

    #[test]
    fn dns_failure_without_working_public_dns_does_not_switch() {
        let dns = item(
            DiagnosticStatus::Fail,
            serde_json::json!({ "server_source": "system", "control_latency_ms": null }),
        );
        let (actions, hints) = generate_recommendations(&pass(), &pass(), &dns, &pass());
        assert_eq!(types(&actions), vec![RepairType::FlushDNSCache]);
        assert_eq!(hints.len(), 1);
    }

    #[test]
    fn unreachable_gateway_gives_physical_hints_and_safest_actions_first() {
        let conn = item(DiagnosticStatus::Fail, serde_json::json!({}));
        let (actions, hints) = generate_recommendations(&conn, &pass(), &pass(), &pass());
        assert!(hints.iter().any(|h| h.contains("网线")));
        assert_eq!(actions[0].action_type, RepairType::ReleaseRenewIP);
        assert!(actions.windows(2).all(|w| w[0].risk <= w[1].risk));
    }

    #[test]
    fn filtered_icmp_alone_recommends_no_repair() {
        let conn = item(DiagnosticStatus::Warning, serde_json::json!({}));
        let quality = item(DiagnosticStatus::Warning, serde_json::json!({}));
        let (actions, hints) = generate_recommendations(&conn, &pass(), &pass(), &quality);
        assert!(actions.is_empty());
        assert_eq!(hints.len(), 1);
    }

    #[test]
    fn everything_failing_keeps_the_low_risk_flush() {
        let dns = item(
            DiagnosticStatus::Fail,
            serde_json::json!({ "server_source": "system", "control_latency_ms": 9.0 }),
        );
        let fail = item(DiagnosticStatus::Fail, serde_json::json!({}));
        let (actions, _) = generate_recommendations(&fail, &fail, &dns, &fail);
        assert_eq!(actions.len(), 3);
        assert_eq!(actions[0].action_type, RepairType::FlushDNSCache);
        assert!(!types(&actions).contains(&RepairType::ResetNetworkStack));
    }

    #[test]
    fn capabilities_match_platform_support() {
        let caps = get_repair_capabilities();
        let ipv6 = caps.iter().find(|c| c.fix_type == "toggle_ipv6").unwrap();
        assert_eq!(ipv6.supported, cfg!(target_os = "macos"));
        assert_eq!(ipv6.reason.is_none(), ipv6.supported);
        assert!(!caps.iter().any(|c| c.fix_type == "restart_network_service"));
        let conn = item(DiagnosticStatus::Pass, serde_json::json!({}));
        let ip = item(DiagnosticStatus::Fail, serde_json::json!({}));
        let (actions, _) = generate_recommendations(&conn, &ip, &pass(), &pass());
        assert_eq!(
            types(&actions).contains(&RepairType::ResetAdapter),
            cfg!(target_os = "macos")
        );
    }
}
