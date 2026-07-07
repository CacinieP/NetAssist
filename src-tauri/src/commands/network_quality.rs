use std::process::Command;
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
    use std::net::IpAddr;
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

    // Block private/local IP addresses to prevent SSRF
    if let Ok(parsed) = reqwest::Url::parse(&url) {
        if let Some(host) = parsed.host_str() {
            if let Ok(ip) = host.parse::<IpAddr>() {
                let is_restricted = match &ip {
                    IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
                    IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local(),
                };
                if is_restricted {
                    return Ok(HttpConnectivityResult {
                        url: url.clone(),
                        success: false,
                        latency_ms: 0.0,
                        status_code: None,
                        error: Some(
                            "Cannot test connectivity to private/local addresses".to_string(),
                        ),
                    });
                }
            }
        }
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

/// Ping a host to measure latency
#[tauri::command]
pub async fn ping(target: String, ipv6: bool) -> Result<PingResult, String> {
    tracing::info!("Starting ping to {} (ipv6={})", target, ipv6);
    // Validate target length to prevent command injection issues
    if target.is_empty() || target.len() > 253 {
        return Err("Invalid target: length must be between 1 and 253 characters".to_string());
    }

    // Validate it's a valid hostname or IP address
    if target.parse::<std::net::IpAddr>().is_err() && !is_valid_hostname(&target) {
        return Err("Target must be a valid IP address or hostname".to_string());
    }

    let count = 4;

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

#[cfg(target_os = "windows")]
async fn ping_windows(target: &str, _ipv6: bool, count: u32) -> Result<PingResult, String> {
    tracing::debug!("Executing Windows ping to {} ({} packets)", target, count);
    let target = target.to_string();
    let count_str = count.to_string();
    let target_clone = target.clone();

    // Run ping in a blocking thread with timeout (10 seconds max)
    let output = timeout(
        Duration::from_secs(10),
        tokio::task::spawn_blocking(move || {
            tracing::trace!("Spawned ping command for {}", target_clone);
            Command::new("ping")
                .args(&["-n", &count_str, &target_clone])
                .output()
        }),
    )
    .await
    .map_err(|_e| {
        tracing::warn!("Ping to {} timed out after 10 seconds", target);
        "Ping timed out after 10 seconds".to_string()
    })?
    .map_err(|e| format!("Ping task join error: {}", e))?
    .map_err(|e| {
        tracing::error!("Ping command failed for {}: {}", target, e);
        format!("Ping failed: {}", e)
    })?;

    tracing::debug!("Ping to {} completed, parsing output", target);

    let content = String::from_utf8_lossy(&output.stdout);

    // Parse Windows ping output
    // Example: "Reply from 142.250.185.46: bytes=32 time=12ms TTL=117"
    let mut latencies = Vec::new();
    let mut packets_received = 0u32;

    for line in content.lines() {
        if line.contains("Reply from") || line.contains("来自") {
            packets_received += 1;

            // Extract latency: English "time=" or Chinese "时间=".
            if let Some(latency) =
                parse_ping_latency(line, "time=").or_else(|| parse_ping_latency(line, "时间="))
            {
                latencies.push(latency);
            }
        }
    }

    let avg_latency = if latencies.is_empty() {
        0.0
    } else {
        latencies.iter().sum::<f64>() / latencies.len() as f64
    };

    let min_latency = latencies.iter().cloned().reduce(f64::min).unwrap_or(0.0);
    let max_latency = latencies.iter().cloned().reduce(f64::max).unwrap_or(0.0);
    let packet_loss_percent = if count > 0 {
        ((count - packets_received) as f64 / count as f64) * 100.0
    } else {
        100.0
    };

    let result = PingResult {
        target: target.to_string(),
        ipv6: false,
        success: packets_received > 0,
        avg_latency_ms: avg_latency,
        min_latency_ms: min_latency,
        max_latency_ms: max_latency,
        packet_loss_percent,
        packets_sent: count,
        packets_received,
    };

    tracing::info!(
        "Ping to {} result: success={}, avg_latency={}ms, packets_received={}/{}",
        target,
        result.success,
        avg_latency,
        packets_received,
        count
    );

    Ok(result)
}

#[cfg(target_os = "linux")]
async fn ping_linux(target: &str, ipv6: bool, count: u32) -> Result<PingResult, String> {
    // -W <sec>: per-reply timeout in seconds on Linux (unlike macOS where it
    //   is milliseconds), so an unreachable host fails fast.
    // -c <n>:   number of packets to send.
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

    // Run ping in a blocking thread with timeout (10 seconds max)
    let output = timeout(
        Duration::from_secs(10),
        tokio::task::spawn_blocking(move || Command::new("ping").args(&args).output()),
    )
    .await
    .map_err(|_| "Ping timed out after 10 seconds".to_string())?
    .map_err(|e| format!("Ping task join error: {}", e))?
    .map_err(|e| format!("Ping failed: {}", e))?;

    let content = String::from_utf8_lossy(&output.stdout);

    // Parse Linux ping output.
    // Example: "64 bytes from 142.250.185.46: icmp_seq=1 ttl=117 time=12.3 ms"
    // Recognize both English ("bytes from") and Chinese ("字节来自") replies.
    let mut latencies = Vec::new();
    let mut packets_received = 0u32;

    for line in content.lines() {
        if line.contains("bytes from") || line.contains("字节来自") {
            packets_received += 1;

            if let Some(latency) =
                parse_ping_latency(line, "time=").or_else(|| parse_ping_latency(line, "时间="))
            {
                latencies.push(latency);
            }
        }
    }

    let avg_latency = if latencies.is_empty() {
        0.0
    } else {
        latencies.iter().sum::<f64>() / latencies.len() as f64
    };

    let min_latency = latencies.iter().cloned().reduce(f64::min).unwrap_or(0.0);
    let max_latency = latencies.iter().cloned().reduce(f64::max).unwrap_or(0.0);
    let packet_loss_percent = if count > 0 {
        ((count - packets_received) as f64 / count as f64) * 100.0
    } else {
        100.0
    };

    Ok(PingResult {
        target: target.to_string(),
        ipv6,
        success: packets_received > 0,
        avg_latency_ms: avg_latency,
        min_latency_ms: min_latency,
        max_latency_ms: max_latency,
        packet_loss_percent,
        packets_sent: count,
        packets_received,
    })
}

#[cfg(target_os = "macos")]
async fn ping_macos(target: &str, ipv6: bool, count: u32) -> Result<PingResult, String> {
    // -W <msec>: per-reply timeout in milliseconds, so an unreachable host
    //   fails fast instead of blocking until the 10s outer timeout.
    // -c <n>:   number of packets to send.
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

    // Run ping in a blocking thread with timeout (10 seconds max)
    let output = timeout(
        Duration::from_secs(10),
        tokio::task::spawn_blocking(move || Command::new("ping").args(&args).output()),
    )
    .await
    .map_err(|_| "Ping timed out after 10 seconds".to_string())?
    .map_err(|e| format!("Ping task join error: {}", e))?
    .map_err(|e| format!("Ping failed: {}", e))?;

    let content = String::from_utf8_lossy(&output.stdout);

    // Parse macOS ping output (similar to Linux).
    // Recognize both English ("bytes from") and Chinese ("字节来自") replies.
    let mut latencies = Vec::new();
    let mut packets_received = 0u32;

    for line in content.lines() {
        if line.contains("bytes from") || line.contains("字节来自") {
            packets_received += 1;

            // English "time=" or Chinese "时间="
            if let Some(latency) =
                parse_ping_latency(line, "time=").or_else(|| parse_ping_latency(line, "时间="))
            {
                latencies.push(latency);
            }
        }
    }

    let avg_latency = if latencies.is_empty() {
        0.0
    } else {
        latencies.iter().sum::<f64>() / latencies.len() as f64
    };

    let min_latency = latencies.iter().cloned().reduce(f64::min).unwrap_or(0.0);
    let max_latency = latencies.iter().cloned().reduce(f64::max).unwrap_or(0.0);
    let packet_loss_percent = if count > 0 {
        ((count - packets_received) as f64 / count as f64) * 100.0
    } else {
        100.0
    };

    Ok(PingResult {
        target: target.to_string(),
        ipv6,
        success: packets_received > 0,
        avg_latency_ms: avg_latency,
        min_latency_ms: min_latency,
        max_latency_ms: max_latency,
        packet_loss_percent,
        packets_sent: count,
        packets_received,
    })
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

    let max_hops = max_hops.unwrap_or(30).min(64);

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

#[cfg(target_os = "windows")]
async fn traceroute_windows(target: &str, max_hops: u32) -> Result<TracerouteResult, String> {
    let target = target.to_string();
    tokio::task::spawn_blocking(move || {
        let output = Command::new("tracert")
            .args(&["-d", "-h", &max_hops.to_string(), &target])
            .output()
            .map_err(|e| format!("Traceroute failed: {}", e))?;

        let content = String::from_utf8_lossy(&output.stdout);
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
                        if part.contains('.') && part.parse::<std::net::Ipv4Addr>().is_ok() {
                            ip_addr = Some(part.to_string());
                            break;
                        }
                    }

                    let success = ip_addr.is_some();
                    hops.push(TracerouteHop {
                        hop_number: hop_num,
                        ip: ip_addr,
                        hostname: None,
                        avg_latency_ms: 0.0,
                        success,
                    });
                }
            }
        }

        let success = !hops.is_empty();
        let total_hops = hops.len() as u32;
        Ok(TracerouteResult {
            target: target.clone(),
            hops,
            success,
            total_hops,
        })
    })
    .await
    .map_err(|e| format!("Traceroute task failed: {}", e))?
}

#[cfg(target_os = "linux")]
async fn traceroute_linux(target: &str, max_hops: u32) -> Result<TracerouteResult, String> {
    let target = target.to_string();
    tokio::task::spawn_blocking(move || {
        // -n: numeric (no DNS)
        // -m: max hops
        // -w: per-hop probe timeout in seconds
        // -q: one probe per hop (simpler latency parsing)
        let output = Command::new("traceroute")
            .args(&[
                "-n",
                "-m",
                &max_hops.to_string(),
                "-w",
                "2",
                "-q",
                "1",
                &target,
            ])
            .output()
            .map_err(|e| format!("Traceroute failed: {}", e))?;

        let content = String::from_utf8_lossy(&output.stdout);
        let mut hops = Vec::new();

        // Format: " <hop> <ip> <latency_ms> ms"  (one probe with -q 1).
        // Silent hop: " <hop> * * *".
        for line in content.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let hop_num = match parts.first().and_then(|s| s.parse::<u32>().ok()) {
                Some(n) => n,
                None => continue,
            };

            if parts.len() < 2 {
                continue;
            }

            if parts[1] == "*" {
                hops.push(TracerouteHop {
                    hop_number: hop_num,
                    ip: None,
                    hostname: None,
                    avg_latency_ms: 0.0,
                    success: false,
                });
                continue;
            }

            let ip_addr = parts[1].to_string();
            let latency = parts
                .iter()
                .skip(2)
                .find_map(|t| t.parse::<f64>().ok())
                .unwrap_or(0.0);

            hops.push(TracerouteHop {
                hop_number: hop_num,
                ip: Some(ip_addr),
                hostname: None,
                avg_latency_ms: latency,
                success: true,
            });
        }

        let success = !hops.is_empty();
        let total_hops = hops.len() as u32;
        Ok(TracerouteResult {
            target: target.clone(),
            hops,
            success,
            total_hops,
        })
    })
    .await
    .map_err(|e| format!("Traceroute task failed: {}", e))?
}

#[cfg(target_os = "macos")]
async fn traceroute_macos(target: &str, max_hops: u32) -> Result<TracerouteResult, String> {
    let target = target.to_string();
    tokio::task::spawn_blocking(move || {
        // -n: no DNS resolution (numeric)
        // -m: max hops
        // -w: per-hop probe timeout in seconds (fail fast on silent hops)
        // -q: one probe per hop (simpler latency parsing)
        let output = Command::new("traceroute")
            .args([
                "-n",
                "-m",
                &max_hops.to_string(),
                "-w",
                "2",
                "-q",
                "1",
                &target,
            ])
            .output()
            .map_err(|e| format!("Traceroute failed: {}", e))?;

        let content = String::from_utf8_lossy(&output.stdout);
        let mut hops = Vec::new();

        // Format: " <hop> <ip> <latency_ms> ms"  (one probe with -q 1).
        // Silent hop: " <hop> * * *".
        for line in content.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let hop_num = match parts.first().and_then(|s| s.parse::<u32>().ok()) {
                Some(n) => n,
                None => continue,
            };

            if parts.len() < 2 {
                continue;
            }

            // "*" means the hop did not respond.
            if parts[1] == "*" {
                hops.push(TracerouteHop {
                    hop_number: hop_num,
                    ip: None,
                    hostname: None,
                    avg_latency_ms: 0.0,
                    success: false,
                });
                continue;
            }

            let ip_addr = parts[1].to_string();
            // The latency value is the first token after the IP that parses as a float.
            let latency = parts
                .iter()
                .skip(2)
                .find_map(|t| t.parse::<f64>().ok())
                .unwrap_or(0.0);

            hops.push(TracerouteHop {
                hop_number: hop_num,
                ip: Some(ip_addr),
                hostname: None,
                avg_latency_ms: latency,
                success: true,
            });
        }

        let hop_count = hops.len();
        Ok(TracerouteResult {
            target: target.clone(),
            hops,
            success: hop_count > 0,
            total_hops: hop_count as u32,
        })
    })
    .await
    .map_err(|e| format!("Traceroute task failed: {}", e))?
}

/// Validate hostname format to prevent command injection
fn is_valid_hostname(hostname: &str) -> bool {
    // Basic hostname validation
    // Hostnames can contain letters, digits, hyphens, and dots
    // But cannot start/end with hyphen or dot, or have consecutive dots
    hostname
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '.')
        && !hostname.starts_with('-')
        && !hostname.ends_with('-')
        && !hostname.starts_with('.')
        && !hostname.ends_with('.')
        && !hostname.contains("..")
}

/// Extract the numeric latency that follows a `key` (e.g. "time=" or "时间=")
/// in a ping reply line. The value runs from immediately after the key up to
/// the first character that is not part of a number (space, 'm' of "ms", etc.).
fn parse_ping_latency(line: &str, key: &str) -> Option<f64> {
    let pos = line.find(key)?;
    let rest = &line[pos + key.len()..];
    // Take leading numeric characters: digits, decimal point, sign.
    let num_str: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    num_str.parse::<f64>().ok()
}
