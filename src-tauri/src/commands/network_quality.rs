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

/// Shared HTTP evidence returned by both the dashboard and overall status.
pub type HttpConnectivityResult = crate::models::network::HttpProbeResult;

/// All diagnostic HTTP requests bypass HTTP/system proxies. VPN/TUN routing
/// remains controlled by the OS. Redirects are not followed so a response is
/// evidence about the requested target, and cannot redirect into a private LAN.
pub(crate) fn direct_http_client(timeout: Duration) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .timeout(timeout)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

pub(crate) fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

pub(crate) fn failed_http_probe(url: &str, kind: &str, error: String) -> HttpConnectivityResult {
    HttpConnectivityResult {
        url: url.to_string(),
        success: false,
        latency_ms: 0.0,
        status_code: None,
        error: Some(error),
        error_kind: Some(kind.to_string()),
        proxy_policy: "no_proxy".to_string(),
        checked_at: chrono::Utc::now().timestamp_millis(),
        address_family: None,
    }
}

pub(crate) async fn probe_http(
    client: &reqwest::Client,
    url: &str,
    method: reqwest::Method,
) -> HttpConnectivityResult {
    let start = std::time::Instant::now();
    let mut result = match client.request(method, url).send().await {
        Ok(response) => {
            let status = response.status();
            let success = status.is_success() || status.is_redirection();
            HttpConnectivityResult {
                url: url.to_string(),
                success,
                latency_ms: 0.0,
                status_code: Some(status.as_u16()),
                error: (!success).then(|| format!("HTTP {}", status.as_u16())),
                error_kind: (!success).then(|| "http_status".to_string()),
                proxy_policy: "no_proxy".to_string(),
                checked_at: chrono::Utc::now().timestamp_millis(),
                address_family: response
                    .remote_addr()
                    .map(|address| if address.is_ipv4() { "ipv4" } else { "ipv6" }.to_string()),
            }
        }
        Err(error) => {
            let kind = if error.is_timeout() {
                "timeout"
            } else if error.is_connect() {
                "connect"
            } else {
                "request"
            };
            failed_http_probe(url, kind, error_chain(&error))
        }
    };
    result.latency_ms = start.elapsed().as_secs_f64() * 1000.0;
    result
}

/// Test one HTTP target. A successful response does not establish that every
/// application, address family, or internet destination is working.
#[tauri::command]
pub async fn test_http_connectivity(url: Option<String>) -> Result<HttpConnectivityResult, String> {
    let url = url.unwrap_or_else(|| "https://www.google.com".to_string());
    let start = std::time::Instant::now();
    let mut result = match timeout(Duration::from_secs(5), test_http_target(&url)).await {
        Ok(result) => result?,
        Err(_) => failed_http_probe(
            &url,
            "timeout",
            "HTTP test timed out (including DNS lookup)".to_string(),
        ),
    };
    result.latency_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(result)
}

async fn test_http_target(url: &str) -> Result<HttpConnectivityResult, String> {
    let parsed = match reqwest::Url::parse(url) {
        Ok(parsed) if url.len() <= 500 && matches!(parsed.scheme(), "http" | "https") => parsed,
        _ => {
            return Ok(failed_http_probe(
                url,
                "invalid_url",
                "Only valid http:// and https:// URLs are allowed".to_string(),
            ))
        }
    };

    // Resolve asynchronously, validate every address, and pin that exact set to
    // the request. This avoids a blocking resolver and a DNS-rebinding window.
    let Some(host) = parsed.host_str() else {
        return Ok(failed_http_probe(
            url,
            "invalid_url",
            "URL has no host".to_string(),
        ));
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let port = parsed.port_or_known_default().unwrap_or(80);
    let addresses: Vec<std::net::SocketAddr> = match timeout(
        Duration::from_secs(5),
        tokio::net::lookup_host((host, port)),
    )
    .await
    {
        Ok(Ok(addresses)) => addresses.collect(),
        Ok(Err(error)) => {
            return Ok(failed_http_probe(
                url,
                "dns",
                format!("DNS lookup failed: {}", error),
            ))
        }
        Err(_) => {
            return Ok(failed_http_probe(
                url,
                "timeout",
                "DNS lookup timed out".to_string(),
            ))
        }
    };
    if addresses.is_empty() {
        return Ok(failed_http_probe(
            url,
            "dns",
            "DNS returned no addresses".to_string(),
        ));
    }
    if addresses
        .iter()
        .any(|address| is_restricted_ip(&address.ip()))
    {
        return Ok(failed_http_probe(
            url,
            "restricted_address",
            "Cannot test connectivity to private/local/loopback addresses".to_string(),
        ));
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    Ok(probe_http(&client, url, reqwest::Method::GET).await)
}

/// True when an IP must never be contacted by a user-supplied HTTP test.
fn is_restricted_ip(ip: &std::net::IpAddr) -> bool {
    if ip.is_unspecified() || ip.is_multicast() {
        return true;
    }
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_broadcast()
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|v4| is_restricted_ip(&v4.into()))
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

        if let Some(latency) = parse_ping_latency(line, "time=")
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
        if let Some(latency) = parse_ping_latency(line, "time=")
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

    #[tokio::test]
    async fn http_probe_preserves_status_and_peer_family_without_following_redirects() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for (status, success) in [(204, true), (302, true), (403, false), (500, false)] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                let received = socket.read(&mut request).await.unwrap();
                assert!(received > 0, "HTTP request must arrive before responding");
                socket.write_all(format!("HTTP/1.1 {} Test\r\nLocation: http://127.0.0.1:1/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", status).as_bytes()).await.unwrap();
            });
            let result = probe_http(
                &direct_http_client(Duration::from_secs(1)).unwrap(),
                &url,
                reqwest::Method::HEAD,
            )
            .await;
            assert_eq!(result.success, success);
            assert_eq!(result.status_code, Some(status));
            assert_eq!(result.address_family.as_deref(), Some("ipv4"));
            assert_eq!(result.proxy_policy, "no_proxy");
            assert_eq!(
                result.error_kind.as_deref(),
                if success { None } else { Some("http_status") }
            );
            assert!(result.checked_at > 0);
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn http_timeout_is_distinct_from_connection_refusal() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let client = direct_http_client(Duration::from_millis(20)).unwrap();
        let result = probe_http(&client, &url, reqwest::Method::GET).await;
        assert_eq!(result.error_kind.as_deref(), Some("timeout"));
        assert_eq!(result.status_code, None);
        drop(listener);
        let result = probe_http(&client, &url, reqwest::Method::GET).await;
        assert_eq!(result.error_kind.as_deref(), Some("connect"));
        assert_eq!(result.address_family, None);
    }

    #[tokio::test]
    async fn http_command_rejects_invalid_and_restricted_destinations() {
        assert_eq!(
            test_http_connectivity(Some("file:///etc/hosts".into()))
                .await
                .unwrap()
                .error_kind
                .as_deref(),
            Some("invalid_url")
        );
        for url in [
            "http://127.0.0.1",
            "http://[::1]",
            "http://[::ffff:127.0.0.1]",
            "http://0.0.0.0",
            "http://224.0.0.1",
        ] {
            assert_eq!(
                test_http_connectivity(Some(url.into()))
                    .await
                    .unwrap()
                    .error_kind
                    .as_deref(),
                Some("restricted_address"),
                "{url}"
            );
        }
    }

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
        let (hop, ip, lat, ok) = parse_traceroute_hop_line("1  192.168.1.1  1.234 ms").unwrap();
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
