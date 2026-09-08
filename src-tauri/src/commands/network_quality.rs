use tokio::time::{timeout, Duration};

/// Traceroute hop result
#[derive(serde::Serialize, Clone)]
pub struct TracerouteHop {
    pub hop_number: u32,
    pub ip: Option<String>,
    pub hostname: Option<String>,
    pub avg_latency_ms: f64,
    pub success: bool,
}

/// Traceroute result
#[derive(serde::Serialize)]
pub struct TracerouteResult {
    pub target: String,
    pub hops: Vec<TracerouteHop>,
    pub success: bool,
    pub total_hops: u32,
}

/// Ping result
#[derive(serde::Serialize)]
pub struct PingResult {
    pub target: String,
    pub ipv6: bool,
    pub success: bool,
    pub avg_latency_ms: f64,
    pub min_latency_ms: f64,
    pub max_latency_ms: f64,
    pub packet_loss_percent: f64,
    pub packets_sent: u32,
    pub packets_received: u32,
}

/// HTTP connectivity test result (more reliable than ICMP)
#[derive(serde::Serialize)]
pub struct HttpConnectivityResult {
    pub url: String,
    pub success: bool,
    pub latency_ms: f64,
    pub status_code: Option<u16>,
    pub error: Option<String>,
}

/// Test HTTP connectivity to a reliable endpoint
#[tauri::command]
pub async fn test_http_connectivity(url: Option<String>) -> Result<HttpConnectivityResult, String> {
    use std::time::Instant;

    let url = url.unwrap_or_else(|| "https://www.google.com".to_string());

    // Validate URL
    if url.is_empty() || url.len() > 500 {
        return Ok(HttpConnectivityResult {
            url: url.clone(),
            success: false,
            latency_ms: 0.0,
            status_code: None,
            error: Some("Invalid URL".to_string()),
        });
    }

    // Enforce HTTP/HTTPS scheme only
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Ok(HttpConnectivityResult {
            url: url.clone(),
            success: false,
            latency_ms: 0.0,
            status_code: None,
            error: Some("Only http:// and https:// URLs are allowed".to_string()),
        });
    }

    // Block requests to private/local/loopback destinations (SSRF guard).
    // The URL host may be a hostname, so resolve it first; also block literal
    // IPv6 link-local and unspecified addresses.
    let blocked = {
        let parsed = reqwest::Url::parse(&url).ok();
        parsed.and_then(|u| u.host_str().map(str::to_string)).map(|host| {
            // Literal IP forms.
            let literal_blocked = host
                .parse::<std::net::IpAddr>()
                .ok()
                .map(|ip| is_restricted_ip(&ip))
                .unwrap_or(false);
            if literal_blocked {
                return true;
            }
            // Hostname: resolve and reject any restricted address.
            host.parse::<std::net::IpAddr>().is_err()
                && (std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), 80)))
                    .map(|mut it| {
                        it.any(|sa| {
                            let ip = sa.ip();
                            is_restricted_ip(&ip)
                                || ip.is_unspecified()
                                || ip.is_multicast()
                        })
                    })
                    .unwrap_or(false)
        })
    };

    if blocked.unwrap_or(false) {
        return Ok(HttpConnectivityResult {
            url: url.clone(),
            success: false,
            latency_ms: 0.0,
            status_code: None,
            error: Some(
                "Cannot test connectivity to private/local/loopback addresses".to_string(),
            ),
        });
    }

    let start = Instant::now();

    // Use reqwest for HTTP request
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            let error_msg = e.to_string();
            let is_timeout = error_msg.contains("timeout") || error_msg.contains("timed out");

            return Ok(HttpConnectivityResult {
                url: url.clone(),
                success: false,
                latency_ms: start.elapsed().as_millis() as f64,
                status_code: None,
                error: Some(if is_timeout {
                    "Request timed out".to_string()
                } else {
                    error_msg
                }),
            });
        }
    };

    let latency = start.elapsed().as_millis() as f64;
    let status = response.status();

    Ok(HttpConnectivityResult {
        url: url.clone(),
        success: status.is_success(),
        latency_ms: latency,
        status_code: Some(status.as_u16()),
        error: if status.is_success() {
            None
        } else {
            Some(format!("HTTP {}", status.as_u16()))
        },
    })
}

/// True when an IP must never be contacted by the connectivity test.
fn is_restricted_ip(ip: &std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_broadcast()
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local()
        }
    }
}

/// Ping a host to measure latency.
///
/// All platform variants use `tokio::process::Command` with `kill_on_drop`,
/// so the child ping/traceroute process is terminated when the outer timeout
/// fires — previously a timed-out ping kept running in a background thread
/// until it finished on its own (up to tens of seconds).
#[tauri::command]
pub async fn ping(target: String, ipv6: bool) -> Result<PingResult, String> {
    // Validate target length to prevent command injection issues
    if target.is_empty() || target.len() > 253 {
        return Err("Invalid target: length must be between 1 and 253 characters".to_string());
    }

    // Validate it's a valid hostname or IP address
    if target.parse::<std::net::IpAddr>().is_err() && !is_valid_hostname(&target) {
        return Err("Target must be a valid IP address or hostname".to_string());
    }

    let count = 4u32;

    #[cfg(target_os = "windows")]
    {
        ping_windows(&target, ipv6, count).await
    }

    #[cfg(target_os = "linux")]
    {
        ping_linux(&target, ipv6, count).await
    }

    #[cfg(target_os = "macos")]
    {
        ping_macos(&target, ipv6, count).await
    }
}

/// Shared aggregation of parsed replies into a PingResult. `received` is
/// clamped to `sent` so duplicate replies (e.g. `DUP!`) can never make
/// `count - received` underflow or produce packet loss above 0%.
fn build_ping_result(
    target: String,
    ipv6: bool,
    sent: u32,
    received: u32,
    latencies: Vec<f64>,
) -> PingResult {
    let received = received.min(sent);
    let loss = if sent > 0 {
        ((sent - received) as f64 / sent as f64) * 100.0
    } else {
        100.0
    };

    let avg = if latencies.is_empty() {
        0.0
    } else {
        latencies.iter().sum::<f64>() / latencies.len() as f64
    };

    PingResult {
        target,
        ipv6,
        success: received > 0 && !latencies.is_empty(),
        avg_latency_ms: avg,
        min_latency_ms: latencies.iter().cloned().reduce(f64::min).unwrap_or(0.0),
        max_latency_ms: latencies.iter().cloned().reduce(f64::max).unwrap_or(0.0),
        packet_loss_percent: loss,
        packets_sent: sent,
        packets_received: received,
    }
}

/// Extract the numeric latency that follows a `key` (e.g. "time=" or "时间=")
/// in a ping reply line. The value runs from immediately after the key up to
/// the first character that is not part of a number.
fn parse_ping_latency(line: &str, key: &str) -> Option<f64> {
    let pos = line.find(key)?;
    let rest = &line[pos + key.len()..];
    let num_str: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    num_str.parse::<f64>().ok()
}

/// Windows ping output uses the console OEM code page (CP936 for zh-CN), so
/// locale text ("来自", "时间=") is NOT decodable via from_utf8_lossy. Instead
/// we parse locale-independent ASCII markers:
/// - a successful reply always carries "TTL=" (unreachable replies such as
///   "Destination host unreachable." do not, so they are never counted);
/// - the latency is the number immediately followed by "ms" ("time=11ms",
///   "时间=11ms" — even in a Chinese locale the 'ms' suffix is ASCII).
#[cfg(target_os = "windows")]
async fn ping_windows(target: &str, ipv6: bool, count: u32) -> Result<PingResult, String> {
    let mut args = vec!["-n".to_string(), count.to_string()];
    if ipv6 {
        args.push("-6".to_string());
    }
    // Per-reply timeout in ms so an unreachable host fails fast.
    args.push("-w".to_string());
    args.push("3000".to_string());
    args.push(target.to_string());

    let out = run_with_timeout("ping", &args, 15).await?;
    let content = String::from_utf8_lossy(&out.stdout);

    let mut latencies = Vec::new();
    let mut packets_received = 0u32;
    for line in content.lines() {
        if !line.contains("TTL=") {
            continue; // timeout lines and "Destination host unreachable" replies
        }
        packets_received += 1;

        // Latency = digits immediately before "ms".
        if let Some(ms_pos) = line.find("ms") {
            let before = &line[..ms_pos];
            let num: String = before
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            if let Ok(v) = num.parse::<f64>() {
                latencies.push(v);
            }
        }
    }

    Ok(build_ping_result(
        target.to_string(),
        ipv6,
        count,
        packets_received,
        latencies,
    ))
}

#[cfg(target_os = "linux")]
async fn ping_linux(target: &str, ipv6: bool, count: u32) -> Result<PingResult, String> {
    // -W <sec>: per-reply timeout in seconds on Linux.
    let mut args = vec![
        "-c".to_string(),
        count.to_string(),
        "-W".to_string(),
        "2".to_string(),
    ];
    if ipv6 {
        args.push("-6".to_string());
    }
    args.push(target.to_string());

    let out = run_with_timeout("ping", &args, 15).await?;
    let content = String::from_utf8_lossy(&out.stdout);

    let mut latencies = Vec::new();
    let mut packets_received = 0u32;
    for line in content.lines() {
        // Count only real replies ("bytes from" / "字节来自"); "DUP!" lines
        // are duplicate replies and are not counted as new packets.
        if line.contains("DUP!") {
            continue;
        }
        let is_reply = line.contains("bytes from") || line.contains("字节来自");
        if !is_reply {
            continue;
        }
        packets_received = packets_received.saturating_add(1);

        if let Some(latency) =
            parse_ping_latency(line, "time=")
            .or_else(|| parse_ping_latency(line, "时间="))
            .or_else(|| parse_ping_latency(line, "time<"))
            .or_else(|| parse_ping_latency(line, "时间<"))
        {
            latencies.push(latency);
        }
    }

    Ok(build_ping_result(
        target.to_string(),
        ipv6,
        count,
        packets_received,
        latencies,
    ))
}

#[cfg(target_os = "macos")]
async fn ping_macos(target: &str, ipv6: bool, count: u32) -> Result<PingResult, String> {
    // -W <msec>: per-reply timeout in MILLISECONDS on macOS.
    let mut args = vec![
        "-c".to_string(),
        count.to_string(),
        "-W".to_string(),
        "2000".to_string(),
    ];
    if ipv6 {
        args.push("-6".to_string());
    }
    args.push(target.to_string());

    let out = run_with_timeout("ping", &args, 15).await?;
    let content = String::from_utf8_lossy(&out.stdout);

    let mut latencies = Vec::new();
    let mut packets_received = 0u32;
    for line in content.lines() {
        if line.contains("DUP!") {
            continue;
        }
        if !(line.contains("bytes from") || line.contains("字节来自")) {
            continue;
        }
        packets_received = packets_received.saturating_add(1);
        if let Some(latency) =
            parse_ping_latency(line, "time=")
            .or_else(|| parse_ping_latency(line, "时间="))
            .or_else(|| parse_ping_latency(line, "time<"))
            .or_else(|| parse_ping_latency(line, "时间<"))
        {
            latencies.push(latency);
        }
    }

    Ok(build_ping_result(
        target.to_string(),
        ipv6,
        count,
        packets_received,
        latencies,
    ))
}

/// Spawn a child process with `kill_on_drop` and a hard outer timeout.
/// Returns the captured output, or an error when the timeout expires
/// (the child is then killed instead of leaking).
async fn run_with_timeout(
    program: &str,
    args: &[String],
    secs: u64,
) -> Result<std::process::Output, String> {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args);
    cmd.kill_on_drop(true);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", program, e))?;

    timeout(Duration::from_secs(secs), child.wait_with_output())
        .await
        .map_err(|_| format!("{} timed out after {} seconds", program, secs))?
        .map_err(|e| format!("{} failed: {}", program, e))
}

/// Traceroute to a host to discover the path
#[tauri::command]
pub async fn traceroute(target: String, max_hops: Option<u32>) -> Result<TracerouteResult, String> {
    // Validate target length to prevent command injection
    if target.is_empty() || target.len() > 253 {
        return Err("Invalid target: length must be between 1 and 253 characters".to_string());
    }

    // Validate it's a valid hostname or IP address
    if target.parse::<std::net::IpAddr>().is_err() && !is_valid_hostname(&target) {
        return Err("Target must be a valid IP address or hostname".to_string());
    }

    let max_hops = match max_hops {
        Some(h) if h > 0 => h.min(64),
        _ => 30,
    };

    #[cfg(target_os = "windows")]
    {
        traceroute_windows(&target, max_hops).await
    }

    #[cfg(target_os = "linux")]
    {
        traceroute_linux(&target, max_hops).await
    }

    #[cfg(target_os = "macos")]
    {
        traceroute_macos(&target, max_hops).await
    }
}

/// Shared hop-line parser for `traceroute -n` output (Linux/macOS format:
/// `<hop> <ip-or-*> <latencies...>`). Returns (hop_number, ip, latency, success).
fn parse_traceroute_hop_line(line: &str) -> Option<(u32, Option<String>, f64, bool)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let hop_num = parts.first()?.parse::<u32>().ok()?;
    if parts.len() < 2 {
        return None;
    }
    if parts[1] == "*" {
        return Some((hop_num, None, 0.0, false));
    }
    let ip_addr = parts[1].to_string();
    // The latency value is the first token after the IP that parses as a float
    // (e.g. " 1  10.0.0.1  1.234 ms" or with 3 probes " 1  10.0.0.1  1.2  1.3  1.4 ms").
    let latency = parts
        .iter()
        .skip(2)
        .find_map(|t| t.parse::<f64>().ok())
        .unwrap_or(0.0);
    Some((hop_num, Some(ip_addr), latency, true))
}

#[cfg(target_os = "windows")]
async fn traceroute_windows(target: &str, max_hops: u32) -> Result<TracerouteResult, String> {
    let args = [
        "-d".to_string(),
        "-h".to_string(),
        max_hops.to_string(),
        // 1s per-probe timeout; the outer timeout below is the real bound.
        "-w".to_string(),
        "1000".to_string(),
        target.to_string(),
    ];

    let out = run_with_timeout("tracert", &args, 90).await?;
    let content = String::from_utf8_lossy(&out.stdout);

    let mut hops = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("Tracing route")
            || trimmed.starts_with("Trace complete")
            || trimmed.contains("* * *")
        {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(hop_num) = parts[0].parse::<u32>() {
                let mut ip_addr = None;
                for part in &parts {
                    // Windows prints IPv4 addresses (e.g. 1.2.3.4) or IPv6.
                    if part.contains(':') && part.parse::<std::net::Ipv6Addr>().is_ok() {
                        ip_addr = Some(part.to_string());
                        break;
                    }
                    if part.parse::<std::net::Ipv4Addr>().is_ok() {
                        ip_addr = Some(part.to_string());
                        break;
                    }
                }
                hops.push(TracerouteHop {
                    hop_number: hop_num,
                    ip: ip_addr.clone(),
                    hostname: None,
                    avg_latency_ms: 0.0,
                    success: ip_addr.is_some(),
                });
            }
        }
    }

    let success = !hops.is_empty();
    let total_hops = hops.len() as u32;
    Ok(TracerouteResult {
        target: target.to_string(),
        hops,
        success,
        total_hops,
    })
}

#[cfg(target_os = "linux")]
async fn traceroute_linux(target: &str, max_hops: u32) -> Result<TracerouteResult, String> {
    let args = [
        "-n".to_string(),
        "-m".to_string(),
        max_hops.to_string(),
        "-w".to_string(),
        "2".to_string(),
        "-q".to_string(),
        "1".to_string(),
        target.to_string(),
    ];

    let out = run_with_timeout("traceroute", &args, 90).await?;
    let content = String::from_utf8_lossy(&out.stdout);

    let mut hops = Vec::new();

    // NOTE: do NOT skip the first line — BSD and Linux traceroute write the
    // "traceroute to ..." header to STDERR, so the first stdout line is hop 1.
    // Non-numeric lines (headers printed to stdout by other builds) are
    // ignored by the hop-number parse below.
    for line in content.lines() {
        if let Some((hop_number, ip, avg_latency_ms, success)) = parse_traceroute_hop_line(line) {
            hops.push(TracerouteHop {
                hop_number,
                ip,
                hostname: None,
                avg_latency_ms,
                success,
            });
        }
    }

    let success = !hops.is_empty();
    let total_hops = hops.len() as u32;
    Ok(TracerouteResult {
        target: target.to_string(),
        hops,
        success,
        total_hops,
    })
}

#[cfg(target_os = "macos")]
async fn traceroute_macos(target: &str, max_hops: u32) -> Result<TracerouteResult, String> {
    let args = [
        "-n".to_string(),
        "-m".to_string(),
        max_hops.to_string(),
        "-w".to_string(),
        "2".to_string(),
        "-q".to_string(),
        "1".to_string(),
        target.to_string(),
    ];

    let out = run_with_timeout("traceroute", &args, 90).await?;
    let content = String::from_utf8_lossy(&out.stdout);

    let mut hops = Vec::new();
    for line in content.lines() {
        if let Some((hop_number, ip, avg_latency_ms, success)) = parse_traceroute_hop_line(line) {
            hops.push(TracerouteHop {
                hop_number,
                ip,
                hostname: None,
                avg_latency_ms,
                success,
            });
        }
    }

    let hop_count = hops.len();
    Ok(TracerouteResult {
        target: target.to_string(),
        hops,
        success: hop_count > 0,
        total_hops: hop_count as u32,
    })
}

/// Validate hostname format to prevent command injection
fn is_valid_hostname(hostname: &str) -> bool {
    if hostname.is_empty() || hostname.len() > 253 {
        return false;
    }
    // ASCII-only, dot/hyphen allowed.
    if !hostname
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.')
    {
        return false;
    }
    // Each label must be non-empty, start/end with alphanumeric and contain
    // no ".." — this also covers trailing '-' inside labels ("bad-.com").
    let mut prev_end = 0usize;
    for (i, b) in hostname.bytes().enumerate() {
        if b == b'.' {
            let label = &hostname[prev_end..i];
            if label.is_empty() || label.starts_with('-') || label.ends_with('-') {
                return false;
            }
            prev_end = i + 1;
        }
    }
    let last = &hostname[prev_end..];
    if last.is_empty() || last.starts_with('-') || last.ends_with('-') {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_ping_result_clamps_received() {
        // Duplicate replies (received > sent) must not underflow or report
        // negative loss.
        let r = build_ping_result("x".into(), false, 4, 7, vec![10.0; 7]);
        assert_eq!(r.packets_received, 4);
        assert_eq!(r.packet_loss_percent, 0.0);
        assert!(r.avg_latency_ms > 0.0);
    }

    #[test]
    fn build_ping_result_full_loss() {
        let r = build_ping_result("x".into(), false, 4, 0, vec![]);
        assert_eq!(r.packet_loss_percent, 100.0);
        assert!(!r.success);
        assert_eq!(r.avg_latency_ms, 0.0);
    }

    #[test]
    fn latency_parsers() {
        assert_eq!(parse_ping_latency("time=12.3 ms", "time="), Some(12.3));
        assert_eq!(parse_ping_latency("时间=5 ms", "时间="), Some(5.0));
        // <1ms replies have no '='; ensure we don't crash (return None, the
        // caller still counts the reply as received).
        assert_eq!(parse_ping_latency("time<1ms", "time="), None);
    }

    #[test]
    fn hostname_validation_rejects_unicode() {
        assert!(is_valid_hostname("example.com"));
        assert!(!is_valid_hostname("exämple.com"));
        assert!(!is_valid_hostname("-bad.com"));
        assert!(!is_valid_hostname("bad-.com"));
        assert!(!is_valid_hostname("a..b"));
    }

    #[test]
    fn traceroute_hop_line() {
        let (hop, ip, lat, ok) =
            parse_traceroute_hop_line("1  192.168.1.1  1.234 ms").unwrap();
        assert_eq!(hop, 1);
        assert_eq!(ip.as_deref(), Some("192.168.1.1"));
        assert!(ok);
        assert!(lat > 1.2);

        let (hop, ip, _, ok) = parse_traceroute_hop_line("2 * * *").unwrap();
        assert_eq!(hop, 2);
        assert_eq!(ip, None);
        assert!(!ok);

        assert!(parse_traceroute_hop_line("traceroute to 8.8.8.8").is_none());
    }
}
