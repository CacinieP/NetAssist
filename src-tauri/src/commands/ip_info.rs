use crate::models::{IPInfo, IPType};
use std::net::IpAddr;

/// Get current IP information with optional GeoIP
#[tauri::command]
pub async fn get_ip_info(include_geoip: Option<bool>) -> Result<IPInfo, String> {
    build_ip_info(include_geoip.unwrap_or(true), false).await
}

/// Local-only IP probe used by diagnostics: never performs external HTTP
/// requests (public IP fetch or GeoIP), so it is fast and offline-safe.
pub(crate) async fn get_local_ip_info_only() -> Result<IPInfo, String> {
    build_ip_info(false, true).await
}

async fn build_ip_info(do_geoip: bool, skip_public_probe: bool) -> Result<IPInfo, String> {
    // Get local IP addresses (internal/LAN) from the real interface list.
    let (local_ipv4_addrs, local_ipv6_addrs) = get_local_addresses();

    // Use first local IPv4, or None if not available
    let local_ipv4 = local_ipv4_addrs.first().cloned();
    // Use first local IPv6, or None if not available
    let local_ipv6 = local_ipv6_addrs.first().cloned();

    // Public IPv4 (external); skipped for the local-only diagnostic probe.
    let public_ipv4 = if skip_public_probe {
        None
    } else {
        get_public_ip().await
    };

    // Use public IP for ipv4 field (external address)
    let display_ipv4 = public_ipv4;
    // For IPv6, display the local (global/ULA) address — a public IPv6 probe
    // would only add latency for the same information.
    let display_ipv6 = local_ipv6.clone();

    // Classify IP types based on what we're displaying
    let ipv4_type =
        display_ipv4
            .as_ref()
            .map_or(IPType::Unknown, |ip| match ip.parse::<IpAddr>() {
                Ok(addr) => classify_ip_type(&addr),
                Err(_) => {
                    tracing::warn!("Failed to parse IPv4 address: {}", ip);
                    IPType::Unknown
                }
            });

    let ipv6_type =
        display_ipv6
            .as_ref()
            .map_or(IPType::Unknown, |ip| match ip.parse::<IpAddr>() {
                Ok(addr) => classify_ip_type(&addr),
                Err(_) => {
                    tracing::warn!("Failed to parse IPv6 address: {}", ip);
                    IPType::Unknown
                }
            });

    // Get GeoIP data only when requested (saves network calls)
    let (ipv4_geoip, ipv6_geoip) = if do_geoip {
        let v4 = if let Some(ref ipv4) = display_ipv4 {
            crate::core::network::geoip::lookup_geoip(ipv4).await
        } else {
            None
        };
        let v6 = if let Some(ref ipv6) = display_ipv6 {
            crate::core::network::geoip::lookup_geoip(ipv6).await
        } else {
            None
        };
        (v4, v6)
    } else {
        (None, None)
    };

    // Check whether the host has both IPv4 and IPv6 connectivity, based on
    // the LOCAL (interface-scoped) addresses — not on the public probes, which
    // can be temporarily absent even when the interface stack is configured.
    let has_ipv4 = local_ipv4.is_some();
    let has_ipv6 = local_ipv6.is_some();

    Ok(IPInfo {
        ipv4: display_ipv4,
        ipv6: display_ipv6,
        local_ipv4,
        local_ipv6,
        ipv4_type,
        ipv6_type,
        ipv4_geoip,
        ipv6_geoip,
        dual_stack_enabled: has_ipv4 && has_ipv6,
        ipv6_priority: false,
    })
}

/// Get overall network status
/// Checks: 1) has local IP, 2) can reach the internet (HTTP probe to reliable endpoint)
#[tauri::command]
pub async fn get_network_status() -> Result<crate::models::NetworkStatus, String> {
    let has_local_ip = !get_local_ipv4_addrs().is_empty();

    if !has_local_ip {
        return Ok(crate::models::NetworkStatus {
            status: "abnormal".to_string(),
            message: "未检测到网络连接".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
        });
    }

    // Probe internet connectivity with a lightweight HEAD request (2s timeout)
    let internet_ok = check_internet_connectivity().await;

    let (status, message) = if internet_ok {
        ("normal".to_string(), "网络连接正常".to_string())
    } else {
        (
            "abnormal".to_string(),
            "已连接路由器但无法访问互联网".to_string(),
        )
    };

    Ok(crate::models::NetworkStatus {
        status,
        message,
        timestamp: chrono::Utc::now().timestamp_millis(),
    })
}

/// Lightweight internet connectivity check via HTTP HEAD probe
async fn check_internet_connectivity() -> bool {
    use tokio::time::{timeout, Duration};

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .no_proxy()
        .build();

    let client = match client {
        Ok(c) => c,
        Err(_) => return false,
    };

    // Try multiple reliable endpoints
    let probes = [
        "https://www.google.com/generate_204",
        "https://cp.cloudflare.com/",
        "https://connectivitycheck.platform.hicloud.com/generate_204",
    ];

    for url in &probes {
        match timeout(Duration::from_secs(2), client.head(*url).send()).await {
            Ok(Ok(resp)) => {
                // Accept any 2xx or 3xx response as "internet is reachable"
                if resp.status().is_success() || resp.status().is_redirection() {
                    return true;
                }
            }
            _ => continue,
        }
    }

    false
}

/// Collect local (LAN) addresses for both families in one pass.
///
/// Enumerates the platform network interfaces (which now work on macOS,
/// Linux and Windows) and keeps only real unicast addresses — never
/// `0.0.0.0`/`::`/unspecified, loopback or link-local. Falls back to the UDP
/// connect trick only when the interface enumeration is unavailable.
fn get_local_addresses() -> (Vec<String>, Vec<String>) {
    let mut ipv4_addrs = Vec::new();
    let mut ipv6_addrs = Vec::new();

    if let Ok(interfaces) = crate::platform::get_network_interfaces() {
        for intf in interfaces {
            if intf.is_loopback || !intf.is_up {
                continue;
            }
            for ip in intf.ipv4_addresses {
                match ip {
                    IpAddr::V4(v4) => {
                        if v4.is_unspecified() || v4.is_loopback() || v4.is_link_local() {
                            continue;
                        }
                        let s = ip.to_string();
                        if !ipv4_addrs.contains(&s) {
                            ipv4_addrs.push(s);
                        }
                    }
                    IpAddr::V6(_) => {}
                }
            }
            for ip in intf.ipv6_addresses {
                match ip {
                    IpAddr::V6(v6) => {
                        if v6.is_unspecified()
                            || v6.is_loopback()
                            || v6.is_unicast_link_local()
                        {
                            continue;
                        }
                        let s = ip.to_string();
                        if !ipv6_addrs.contains(&s) {
                            ipv6_addrs.push(s);
                        }
                    }
                    IpAddr::V4(_) => {}
                }
            }
        }
    }

    // Fallbacks: connect() a UDP socket to derive the source address (no
    // packets are actually sent for UDP connect).
    if ipv4_addrs.is_empty() {
        if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
            if socket.connect("8.8.8.8:53").is_ok() {
                if let Ok(addr) = socket.local_addr() {
                    if let IpAddr::V4(ipv4) = addr.ip() {
                        if !ipv4.is_unspecified() && !ipv4.is_loopback() && !ipv4.is_link_local()
                        {
                            ipv4_addrs.push(ipv4.to_string());
                        }
                    }
                }
            }
        }
    }
    if ipv6_addrs.is_empty() {
        if let Ok(socket) = std::net::UdpSocket::bind("[::]:0") {
            if socket.connect("[2001:4860:4860::8888]:53").is_ok() {
                if let Ok(addr) = socket.local_addr() {
                    if let IpAddr::V6(ipv6) = addr.ip() {
                        if !ipv6.is_unspecified()
                            && !ipv6.is_loopback()
                            && !ipv6.is_unicast_link_local()
                        {
                            ipv6_addrs.push(ipv6.to_string());
                        }
                    }
                }
            }
        }
    }

    (ipv4_addrs, ipv6_addrs)
}

/// Get the local IPv4 addresses (kept for network-status checks).
fn get_local_ipv4_addrs() -> Vec<String> {
    get_local_addresses().0
}

/// Get public IPv4 address (using external API) with overall timeout
async fn get_public_ip() -> Option<String> {
    use tokio::time::{timeout, Duration};

    // IPv4-only services for better GeoIP coverage
    let services = [
        "https://api4.ipify.org",     // IPv4-only endpoint
        "https://ipv4.icanhazip.com", // IPv4-only endpoint
        "https://ifconfig.me/ip",     // Returns IPv4 when available
    ];

    tracing::info!(
        "Attempting to fetch public IPv4 from {} services",
        services.len()
    );

    // Increase overall timeout to 15 seconds to account for proxy delays
    let overall_timeout = timeout(Duration::from_secs(15), async {
        for (idx, service) in services.iter().enumerate() {
            tracing::debug!("Trying service {}/{}: {}", idx + 1, services.len(), service);

            // Increase per-service timeout to 5 seconds for proxy scenarios
            let result = timeout(
                Duration::from_secs(5),
                fetch_public_ipv4_from_service(service),
            )
            .await;

            match result {
                Ok(Ok(ip)) => {
                    tracing::info!("Successfully fetched public IPv4: {} from {}", ip, service);
                    return Some(ip);
                }
                Ok(Err(e)) => {
                    tracing::warn!("Service {} returned error: {}", service, e);
                }
                Err(_) => {
                    tracing::warn!("Service {} timed out after 5 seconds", service);
                }
            }
        }
        tracing::error!("All IPv4 services failed");
        None as Option<String>
    })
    .await;

    match &overall_timeout {
        Ok(Some(ip)) => tracing::info!("Public IPv4 retrieved: {}", ip),
        Ok(None) => tracing::error!("No public IPv4 could be retrieved"),
        Err(_) => tracing::error!("Overall timeout (15s) reached while fetching public IPv4"),
    }

    overall_timeout.ok().flatten()
}

/// Fetch public IPv4 from a specific service with validation
async fn fetch_public_ipv4_from_service(
    service: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // Build client with proxy bypass and increased timeout
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .no_proxy()
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    tracing::debug!("Sending request to {}", service);

    let response = client
        .get(service)
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let status = response.status();
    let ip = response
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    let ip = ip.trim();

    tracing::debug!(
        "Got response from {}: status={}, body={}",
        service,
        status,
        ip
    );

    // Validate it's a valid IPv4 address (not IPv6)
    match ip.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(_)) => Ok(ip.to_string()),
        Ok(std::net::IpAddr::V6(_)) => Err(format!("Expected IPv4 but got IPv6: {}", ip).into()),
        Err(_) => Err(format!("Invalid IP address returned: {}", ip).into()),
    }
}

/// Classify IP address type
fn classify_ip_type(ip: &IpAddr) -> IPType {
    match ip {
        IpAddr::V4(ipv4) => {
            if ipv4.is_loopback() {
                return IPType::Loopback;
            }
            if ipv4.is_private() {
                return IPType::Private;
            }
            IPType::Public
        }
        IpAddr::V6(ipv6) => {
            if ipv6.is_loopback() {
                return IPType::Loopback;
            }
            if ipv6.is_unicast_link_local() {
                return IPType::LinkLocal;
            }
            if ipv6.is_unique_local() {
                return IPType::Private;
            }
            IPType::Public
        }
    }
}
