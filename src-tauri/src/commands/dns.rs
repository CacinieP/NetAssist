use crate::models::DNSStats;
use std::time::Instant;
use trust_dns_client::rr::Name;
use tokio::net::UdpSocket;
use std::net::SocketAddr;
use std::str::FromStr;

/// Get system DNS servers
#[tauri::command]
pub async fn get_dns_servers() -> Result<Vec<String>, String> {
    crate::platform::get_dns_servers()
        .map_err(|e| e.to_string())
}

/// Test DNS server response time using actual DNS queries
#[tauri::command]
pub async fn test_dns(server: String) -> Result<DNSStats, String> {
    // Validate server address length to prevent buffer overflow
    if server.len() > 253 {
        return Err("DNS server address too long (max 253 characters)".to_string());
    }

    // Parse server address, add default port if needed
    let server_addr = if server.contains(':') {
        server.clone()
    } else {
        format!("{}:53", server)
    };

    let socket_addr: SocketAddr = server_addr.parse()
        .map_err(|e| format!("Invalid DNS server address: {}", e))?;

    // Test multiple queries for accuracy
    let mut latencies = Vec::new();
    let mut successful_queries = 0u64;
    let total_queries = 5u64;

    // Test domains to query
    let test_domains = vec![
        "google.com",
        "cloudflare.com",
        "example.com",
    ];

    for i in 0..total_queries {
        let domain = test_domains[i as usize % test_domains.len()];

        match perform_dns_query(socket_addr, domain).await {
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
        success_rate: success_rate,
        total_queries,
        failed_queries: total_queries - successful_queries,
        cache_hit_rate: 0.0,
    })
}

/// Perform actual DNS query to test the server
async fn perform_dns_query(server: SocketAddr, domain: &str) -> Result<f64, String> {
    let start = Instant::now();

    // Validate domain name format
    let _name = Name::from_str(domain)
        .map_err(|e| format!("Invalid domain name: {}", e))?;

    // Create UDP socket for DNS query
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .map_err(|e| format!("Failed to bind socket: {}", e))?;

    // Set timeout
    socket.set_ttl(64)
        .map_err(|e| format!("Failed to set TTL: {}", e))?;

    // Connect to DNS server
    let timeout_duration = tokio::time::Duration::from_secs(3);
    let _ = tokio::time::timeout(timeout_duration, socket.connect(server))
        .await
        .map_err(|_| "DNS server connection timeout".to_string())?
        .map_err(|e| format!("Failed to connect to DNS server: {}", e))?;

    // Build DNS query packet
    let mut query_packet = vec![0u8; 512];
    let query_len = build_dns_query(&mut query_packet, domain)?;

    // Send query
    socket.send(&query_packet[..query_len])
        .await
        .map_err(|e| format!("Failed to send DNS query: {}", e))?;

    // Receive response
    let mut response_buffer = vec![0u8; 512];
    let timeout_future = tokio::time::timeout(timeout_duration, socket.recv(&mut response_buffer));
    let bytes_received = timeout_future.await
        .map_err(|_| "DNS response timeout".to_string())?
        .map_err(|e| format!("Failed to receive DNS response: {}", e))?;

    // Validate response buffer length before accessing
    if bytes_received < 12 {
        return Err("Invalid DNS response length (too short)".to_string());
    }

    // Check DNS response header
    let response_code = response_buffer[3] & 0x0F;
    if response_code != 0 {
        return Err(format!("DNS query failed with response code: {}", response_code));
    }

    // Check if we have answers (safely access buffer)
    if bytes_received < 8 {
        return Err("DNS response too short for answer count".to_string());
    }
    let answer_count = u16::from_be_bytes([response_buffer[6], response_buffer[7]]);
    if answer_count == 0 {
        return Err("DNS server returned no answers".to_string());
    }

    Ok(start.elapsed().as_millis() as f64)
}

/// Build a simple DNS query packet
fn build_dns_query(buffer: &mut [u8], domain: &str) -> Result<usize, String> {
    if buffer.len() < 12 {
        return Err("Buffer too small".to_string());
    }

    // DNS Header
    buffer[0] = 0x12; // Transaction ID (high byte)
    buffer[1] = 0x34; // Transaction ID (low byte)
    buffer[2] = 0x01; // Flags: standard query
    buffer[3] = 0x00; // Flags: 0 questions, 0 answers, 0 authority, 0 additional
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
