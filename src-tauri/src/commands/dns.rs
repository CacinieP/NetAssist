use crate::models::DNSStats;
use hickory_proto::rr::Name;
use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Instant;
use tokio::net::UdpSocket;

/// Get system DNS servers
#[tauri::command]
pub async fn get_dns_servers() -> Result<Vec<String>, String> {
    crate::platform::get_dns_servers().map_err(|e| e.to_string())
}

/// Test DNS server response time using actual DNS queries
#[tauri::command]
pub async fn test_dns(server: String) -> Result<DNSStats, String> {
    // The server must be an IP literal (IPv4 or IPv6). Hostnames were
    // previously accepted here but then failed to parse as SocketAddr, so
    // every named server would report 100% failure.
    if server.parse::<std::net::IpAddr>().is_err() {
        // Allow bracket-wrapped IPv6 ("[::1]") — strip the brackets.
        let stripped = server.trim_start_matches('[').trim_end_matches(']');
        if stripped.parse::<std::net::IpAddr>().is_err() {
            return Err(format!("Invalid DNS server address: {}", server));
        }
    }

    // Normalize to a SocketAddr (IPv4, IPv6, bracketed or not, default :53).
    let server_addr: SocketAddr = {
        let raw = server.trim();
        let candidate = if raw.parse::<SocketAddr>().is_ok() {
            raw.to_string()
        } else if raw.starts_with('[') {
            // Bracket-wrapped (e.g. "[::1]") without a port.
            format!("{}:53", raw)
        } else if raw.contains(':') {
            // Bare IPv6 without a port → "[::1]:53".
            format!("[{}]:53", raw)
        } else {
            // IPv4 or hostname → "8.8.8.8:53". (Hostnames are rejected below.)
            format!("{}:53", raw)
        };
        match candidate.parse::<SocketAddr>() {
            Ok(addr) => addr,
            Err(e) => return Err(format!("Invalid DNS server address: {}", e)),
        }
    };

    // Test multiple queries for accuracy
    let mut latencies = Vec::new();
    let mut successful_queries = 0u64;
    let total_queries = 5u64;

    // Test domains to query
    let test_domains = ["google.com", "cloudflare.com", "example.com"];

    for i in 0..total_queries {
        let domain = test_domains[i as usize % test_domains.len()];

        match perform_dns_query(server_addr, domain).await {
            Ok(latency) => {
                successful_queries += 1;
                latencies.push(latency);
            }
            Err(e) => {
                tracing::warn!("DNS query failed for {} via {}: {}", domain, server, e);
            }
        }

        // Small delay between queries
        if i < total_queries - 1 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    let avg_latency = if latencies.is_empty() {
        0.0
    } else {
        latencies.iter().sum::<f64>() / latencies.len() as f64
    };

    let success_rate = successful_queries as f64 / total_queries as f64;

    Ok(DNSStats {
        server: server.clone(),
        avg_latency_ms: avg_latency,
        success_rate,
        total_queries,
        failed_queries: total_queries - successful_queries,
        cache_hit_rate: 0.0,
    })
}

/// Perform actual DNS query to test the server
async fn perform_dns_query(server: SocketAddr, domain: &str) -> Result<f64, String> {
    let start = Instant::now();

    // Validate domain name format
    let _name = Name::from_str(domain).map_err(|e| format!("Invalid domain name: {}", e))?;

    // Bind a UDP socket of the SAME family as the server (an IPv4-only socket
    // cannot connect() to an IPv6 DNS server, which previously made every
    // IPv6 DNS test fail).
    let bind_addr = if server.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
    let socket = UdpSocket::bind(bind_addr)
        .await
        .map_err(|e| format!("Failed to bind socket: {}", e))?;

    // Give the OS 64s TTL for outbound packets (not a "timeout"; the timeout
    // is applied separately around connect/send/recv below).
    socket
        .set_ttl(64)
        .map_err(|e| format!("Failed to set TTL: {}", e))?;

    // Connect to DNS server
    let timeout_duration = tokio::time::Duration::from_secs(3);
    tokio::time::timeout(timeout_duration, socket.connect(server))
        .await
        .map_err(|_| "DNS server connection timeout".to_string())?
        .map_err(|e| format!("Failed to connect to DNS server: {}", e))?;

    // Build DNS query packet with a transaction id derived from the clock
    // (a fixed 0x1234 id is trivially spoofable and never validated anyway).
    let txid: u16 = (start.elapsed().as_nanos() & 0xFFFF) as u16;
    let mut query_packet = vec![0u8; 512];
    let query_len = build_dns_query(&mut query_packet, domain, txid)?;

    // Send query
    socket
        .send(&query_packet[..query_len])
        .await
        .map_err(|e| format!("Failed to send DNS query: {}", e))?;

    // Receive response
    let mut response_buffer = vec![0u8; 512];
    let timeout_future = tokio::time::timeout(timeout_duration, socket.recv(&mut response_buffer));
    let bytes_received = timeout_future
        .await
        .map_err(|_| "DNS response timeout".to_string())?
        .map_err(|e| format!("Failed to receive DNS response: {}", e))?;

    // Validate response buffer length (header is 12 bytes; anything shorter
    // is not a DNS packet).
    if bytes_received < 12 {
        return Err("Invalid DNS response length (too short)".to_string());
    }

    // Match the response to OUR query (transaction id + QR flag).
    if response_buffer[0] != (txid >> 8) as u8 || response_buffer[1] != (txid & 0xFF) as u8 {
        return Err("DNS response transaction ID mismatch".to_string());
    }
    if response_buffer[2] & 0x80 == 0 {
        return Err("DNS response is not a response (QR bit not set)".to_string());
    }

    // Check DNS response header
    let response_code = response_buffer[3] & 0x0F;
    if response_code != 0 {
        return Err(format!(
            "DNS query failed with response code: {}",
            response_code
        ));
    }

    // Check if we have answers
    let answer_count = u16::from_be_bytes([response_buffer[6], response_buffer[7]]);
    if answer_count == 0 {
        return Err("DNS server returned no answers".to_string());
    }

    // Sub-millisecond precision: as_millis() truncates <1ms replies to 0,
    // which made fast (router-cached) servers look like failures downstream.
    Ok(start.elapsed().as_secs_f64() * 1000.0)
}

/// Build a simple DNS query packet
fn build_dns_query(buffer: &mut [u8], domain: &str, txid: u16) -> Result<usize, String> {
    if buffer.len() < 12 {
        return Err("Buffer too small".to_string());
    }

    // DNS Header
    buffer[0] = (txid >> 8) as u8; // Transaction ID (high byte)
    buffer[1] = (txid & 0xFF) as u8; // Transaction ID (low byte)
    buffer[2] = 0x01; // Flags: standard query
    buffer[3] = 0x00;
    buffer[4] = 0x00; // Questions: high byte
    buffer[5] = 0x01; // Questions: low byte (1 question)
    buffer[6] = 0x00; // Answer RRs: high byte
    buffer[7] = 0x00; // Answer RRs: low byte
    buffer[8] = 0x00; // Authority RRs: high byte
    buffer[9] = 0x00; // Authority RRs: low byte
    buffer[10] = 0x00; // Additional RRs: high byte
    buffer[11] = 0x00; // Additional RRs: low byte

    let mut pos = 12;

    // Encode domain name
    for label in domain.split('.') {
        if label.is_empty() {
            continue;
        }
        let label_bytes = label.as_bytes();
        if label_bytes.len() > 63 {
            return Err("Domain label too long".to_string());
        }
        if pos + 1 + label_bytes.len() > buffer.len() {
            return Err("Buffer too small for domain name".to_string());
        }
        buffer[pos] = label_bytes.len() as u8;
        pos += 1;
        buffer[pos..pos + label_bytes.len()].copy_from_slice(label_bytes);
        pos += label_bytes.len();
    }

    // End of domain name
    if pos + 1 > buffer.len() {
        return Err("Buffer too small for domain terminator".to_string());
    }
    buffer[pos] = 0; // Root label
    pos += 1;

    // QTYPE (A record = 1)
    if pos + 2 > buffer.len() {
        return Err("Buffer too small for QTYPE".to_string());
    }
    buffer[pos] = 0x00;
    buffer[pos + 1] = 0x01; // A record
    pos += 2;

    // QCLASS (IN = 1)
    if pos + 2 > buffer.len() {
        return Err("Buffer too small for QCLASS".to_string());
    }
    buffer[pos] = 0x00;
    buffer[pos + 1] = 0x01; // IN
    pos += 2;

    Ok(pos)
}
