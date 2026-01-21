#![allow(dead_code)]

use crate::models::GeoIPInfo;
use std::collections::HashMap;

/// Check if IPv6 address is link-local (fe80::/10)
fn is_ipv6_link_local_check(ipv6: &std::net::Ipv6Addr) -> bool {
    let segments = ipv6.segments();
    segments[0] == 0xfe80 && (segments[1] & 0xc000) == 0x8000
}

/// Get GeoIP information for an IP address
pub async fn lookup_geoip(ip: &str) -> Option<GeoIPInfo> {
    // Check if it's a local/private IP
    if is_local_ip(ip) {
        return Some(GeoIPInfo {
            country: "本地网络".to_string(),
            region: "-".to_string(),
            city: "-".to_string(),
            latitude: None,
            longitude: None,
        });
    }

    // Use online API to get GeoIP information
    match lookup_geoip_online(ip).await {
        Ok(info) => Some(info),
        Err(_) => None,
    }
}

/// Check if IP is local/private
fn is_local_ip(ip: &str) -> bool {
    match ip.parse::<std::net::IpAddr>() {
        Ok(addr) => {
            match addr {
                std::net::IpAddr::V4(ipv4) => {
                    ipv4.is_loopback() || ipv4.is_private() || ipv4.is_link_local()
                }
                std::net::IpAddr::V6(ipv6) => {
                    ipv6.is_loopback() || ipv6.is_unique_local() || is_ipv6_link_local_check(&ipv6)
                }
            }
        }
        Err(_) => false,
    }
}

/// Look up GeoIP using online API with fallback
async fn lookup_geoip_online(ip: &str) -> Result<GeoIPInfo, String> {
    tracing::info!("Looking up GeoIP for {}", ip);

    // List of GeoIP services with different APIs for redundancy
    let services: Vec<(&str, &str)> = vec![
        // ip-api.com - HTTP for free tier (HTTPS is paid only)
        ("http://ip-api.com/json/", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"),
        // ipapi.co - Free tier, no key required
        ("https://ipapi.co/json/", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"),
        // ipwhois.app as fallback
        ("https://ipwhois.app/json/", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"),
    ];

    for (idx, (base_url, user_agent)) in services.iter().enumerate() {
        let url = format!("{}{}", base_url, ip);
        tracing::debug!("Trying GeoIP service {}/{}: {}", idx + 1, services.len(), url);

        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .no_proxy()
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to build HTTP client for GeoIP: {}", e);
                continue;
            }
        };

        let resp = match client.get(&url)
            .header("User-Agent", *user_agent)
            .send()
            .await
        {
            Ok(r) => {
                tracing::debug!("GeoIP request status: {}", r.status());
                r
            }
            Err(e) => {
                tracing::warn!("GeoIP request failed for {}: {}", base_url, e);
                continue;
            }
        };

        let _status = resp.status();
        let data = match resp.json::<serde_json::Value>().await {
            Ok(d) => {
                tracing::debug!("GeoIP response parsed successfully");
                d
            }
            Err(e) => {
                tracing::warn!("Failed to parse GeoIP JSON response from {}: {}", base_url, e);
                continue;
            }
        };

        // Try parsing based on service
        let result = if base_url.contains("ip-api.com") {
            parse_ipapi_response(data)
        } else if base_url.contains("ipapi.co") {
            parse_ipapico_response(data)
        } else if base_url.contains("ipwhois.app") {
            parse_ipwhois_response(data)
        } else {
            continue;
        };

        match &result {
            Ok(geoip) if geoip.country != "未知" && geoip.country != "-" => {
                tracing::info!("GeoIP lookup success for {}: {} (from {})", ip, geoip.country, base_url);
                return result;
            }
            Ok(geoip) => {
                tracing::debug!("GeoIP service {} returned: country='{}', region='{}', city='{}', trying next",
                    base_url, geoip.country, geoip.region, geoip.city);
            }
            Err(e) => {
                tracing::warn!("GeoIP service {} returned error: {}, trying next", base_url, e);
            }
        }
    }

    tracing::warn!("All GeoIP services failed for {}", ip);
    Ok(GeoIPInfo {
        country: "未知".to_string(),
        region: "-".to_string(),
        city: "-".to_string(),
        latitude: None,
        longitude: None,
    })
}

/// Parse response from ip-api.com
fn parse_ipapi_response(data: serde_json::Value) -> Result<GeoIPInfo, String> {
    tracing::debug!("ip-api.com response: {}", data);

    if data["status"].as_str() != Some("success") {
        let message = data["message"].as_str().unwrap_or("unknown error").to_string();
        tracing::warn!("ip-api.com returned status='{}', message='{}", data["status"].as_str().unwrap_or("none"), message);
        return Ok(GeoIPInfo {
            country: "未知".to_string(),
            region: "-".to_string(),
            city: "-".to_string(),
            latitude: None,
            longitude: None,
        });
    }

    let country = data["country"].as_str().unwrap_or("-").to_string();
    let region = data["regionName"].as_str().unwrap_or("-").to_string();
    let city = data["city"].as_str().unwrap_or("-").to_string();

    tracing::debug!("ip-api.com parsed: country={}, region={}, city={}", country, region, city);

    Ok(GeoIPInfo {
        country,
        region,
        city,
        latitude: data["lat"].as_f64(),
        longitude: data["lon"].as_f64(),
    })
}

/// Parse response from ipwhois.app
fn parse_ipwhois_response(data: serde_json::Value) -> Result<GeoIPInfo, String> {
    tracing::debug!("ipwhois.app response: {}", data);

    // ipwhois.app returns { success: true/false, ... } on error
    if data["success"].as_bool() == Some(false) {
        tracing::warn!("ipwhois.app returned success=false");
        return Ok(GeoIPInfo {
            country: "未知".to_string(),
            region: "-".to_string(),
            city: "-".to_string(),
            latitude: None,
            longitude: None,
        });
    }

    let country = data["country"].as_str().unwrap_or("-").to_string();
    let region = data["region"].as_str().unwrap_or("-").to_string();
    let city = data["city"].as_str().unwrap_or("-").to_string();

    tracing::debug!("ipwhois.app parsed: country={}, region={}, city={}", country, region, city);

    Ok(GeoIPInfo {
        country,
        region,
        city,
        latitude: data["latitude"].as_f64(),
        longitude: data["longitude"].as_f64(),
    })
}

/// Parse response from ipapi.co
fn parse_ipapico_response(data: serde_json::Value) -> Result<GeoIPInfo, String> {
    tracing::debug!("ipapi.co response: {}", data);

    // ipapi.co returns error reason on failure
    if let Some(_reason) = data["error"].as_str() {
        tracing::warn!("ipapi.co returned error");
        return Ok(GeoIPInfo {
            country: "未知".to_string(),
            region: "-".to_string(),
            city: "-".to_string(),
            latitude: None,
            longitude: None,
        });
    }

    let country = data["country_name"].as_str().or_else(|| data["country"].as_str()).unwrap_or("-").to_string();
    let region = data["region"].as_str().or_else(|| data["region_name"].as_str()).unwrap_or("-").to_string();
    let city = data["city"].as_str().unwrap_or("-").to_string();

    tracing::debug!("ipapi.co parsed: country={}, region={}, city={}", country, region, city);

    Ok(GeoIPInfo {
        country,
        region,
        city,
        latitude: data["latitude"].as_f64(),
        longitude: data["longitude"].as_f64(),
    })
}

/// Get GeoIP information for multiple IPs
pub async fn lookup_geoip_batch(ips: Vec<String>) -> HashMap<String, GeoIPInfo> {
    let mut results = HashMap::new();

    for ip in ips {
        if let Some(info) = lookup_geoip(&ip).await {
            results.insert(ip, info);
        }
    }

    results
}
