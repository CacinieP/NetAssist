use super::network_quality::{direct_http_client, error_chain, failed_http_probe, probe_http};
use crate::models::network::{HttpProbeResult, LocalAddress, PublicIpProbe};
use crate::models::{IPInfo, IPType, NetworkStatus};
use std::{
    future::Future,
    net::IpAddr,
    sync::OnceLock,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const CONNECTIVITY_TARGETS: [&str; 3] = [
    "https://www.google.com/generate_204",
    "https://cp.cloudflare.com/",
    "https://connectivitycheck.platform.hicloud.com/generate_204",
];
const PUBLIC_IP_SERVICES: [&str; 3] = [
    "https://api4.ipify.org",
    "https://ipv4.icanhazip.com",
    "https://ifconfig.me/ip",
];
const PUBLIC_IP_SUCCESS_TTL: Duration = Duration::from_secs(15);
const PUBLIC_IP_FAILURE_TTL: Duration = Duration::from_secs(15);

#[derive(Clone, Copy)]
enum IpProbeMode {
    LocalOnly,
    External { include_geoip: bool },
}

#[tauri::command]
pub async fn get_ip_info(include_geoip: Option<bool>) -> Result<IPInfo, String> {
    build_ip_info(IpProbeMode::External {
        include_geoip: include_geoip.unwrap_or(true),
    })
    .await
}

/// Local-only diagnostics never wait on external HTTP requests.
#[tauri::command]
pub async fn get_local_ip_info_only() -> Result<IPInfo, String> {
    build_ip_info(IpProbeMode::LocalOnly).await
}

async fn build_ip_info(mode: IpProbeMode) -> Result<IPInfo, String> {
    build_ip_info_with_probes(
        mode,
        get_local_addresses(),
        get_public_ip_cached,
        |ip| async move { crate::core::network::geoip::lookup_geoip(&ip).await },
    )
    .await
}

async fn build_ip_info_with_probes<P, PFut, G, GFut>(
    mode: IpProbeMode,
    local_addresses: Vec<LocalAddress>,
    public_lookup: P,
    mut geoip_lookup: G,
) -> Result<IPInfo, String>
where
    P: FnOnce() -> PFut,
    PFut: Future<Output = PublicIpObservation>,
    G: FnMut(String) -> GFut,
    GFut: Future<Output = Option<crate::models::GeoIPInfo>>,
{
    let (do_geoip, skip_public_probe) = match mode {
        IpProbeMode::LocalOnly => (false, true),
        IpProbeMode::External { include_geoip } => (include_geoip, false),
    };
    let selected_ipv4 = local_addresses
        .iter()
        .find(|address| address.family == "ipv4");
    let selected_ipv6 = local_addresses
        .iter()
        .find(|address| address.family == "ipv6");
    let local_ipv4 = selected_ipv4.map(|address| address.address.clone());
    let local_ipv6 = selected_ipv6.map(|address| address.address.clone());
    let ipv6_interface = selected_ipv6.and_then(|address| address.interface.clone());
    let ipv6_source = selected_ipv6.map(|address| address.source.clone());
    let public = if skip_public_probe {
        PublicIpObservation {
            ip: None,
            probe: PublicIpProbe::default(),
        }
    } else {
        public_lookup().await
    };
    let display_ipv4 = public.ip;
    // Compatibility field: this is local configuration, not a public IPv6 probe
    // or proof that a particular destination uses this interface/address.
    let display_ipv6 = local_ipv6.clone();
    let classify = |ip: &Option<String>| {
        ip.as_ref()
            .and_then(|ip| ip.parse::<IpAddr>().ok())
            .map_or(IPType::Unknown, |ip| classify_ip_type(&ip))
    };
    let ipv4_type = classify(&display_ipv4);
    let ipv6_type = classify(&display_ipv6);
    let (ipv4_geoip, ipv6_geoip) = if do_geoip {
        let v4 = if let Some(ref ip) = display_ipv4 {
            geoip_lookup(ip.clone()).await
        } else {
            None
        };
        let v6 = if let Some(ref ip) = display_ipv6 {
            geoip_lookup(ip.clone()).await
        } else {
            None
        };
        (v4, v6)
    } else {
        (None, None)
    };
    let dual_stack_enabled = local_ipv4.is_some() && local_ipv6.is_some();
    Ok(IPInfo {
        ipv4: display_ipv4,
        ipv6: display_ipv6,
        local_ipv4,
        local_ipv6,
        ipv6_interface,
        ipv6_source,
        local_addresses,
        public_ipv4_probe: public.probe,
        ipv4_type,
        ipv6_type,
        ipv4_geoip,
        ipv6_geoip,
        dual_stack_enabled,
        ipv6_priority: false,
    })
}

#[tauri::command]
pub async fn get_network_status() -> Result<NetworkStatus, String> {
    Ok(network_status_for_addresses(&get_local_addresses(), check_connectivity).await)
}

/// Either family permits probing; address configuration is not connectivity.
async fn network_status_for_addresses<F, Fut>(addresses: &[LocalAddress], probe: F) -> NetworkStatus
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Vec<HttpProbeResult>>,
{
    if addresses.is_empty() {
        return NetworkStatus {
            status: "abnormal".to_string(),
            message: "未检测到可用的本地 IPv4/IPv6 地址，未执行 HTTP 探测".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            probes: Vec::new(),
        };
    }
    summarize_probes(probe().await)
}

fn summarize_probes(probes: Vec<HttpProbeResult>) -> NetworkStatus {
    let succeeded = probes.iter().filter(|probe| probe.success).count();
    let message = if succeeded > 0 {
        format!(
            "HTTP 探测目标 {}/{} 可达；仅代表本次所测目标",
            succeeded,
            probes.len()
        )
    } else {
        format!(
            "本次 {} 个 HTTP 探测目标均未成功；不能据此判断所有应用或互联网不可用",
            probes.len()
        )
    };
    NetworkStatus {
        status: if succeeded > 0 { "normal" } else { "abnormal" }.to_string(),
        message,
        timestamp: chrono::Utc::now().timestamp_millis(),
        probes,
    }
}

async fn check_connectivity() -> Vec<HttpProbeResult> {
    let client = match direct_http_client(Duration::from_secs(2)) {
        Ok(client) => client,
        Err(error) => {
            return CONNECTIVITY_TARGETS
                .iter()
                .map(|url| failed_http_probe(url, "request", error.to_string()))
                .collect()
        }
    };
    // Always retain all three observations; concurrent requests bound the whole
    // batch to a single request timeout without hiding failures after a success.
    let (a, b, c) = tokio::join!(
        probe_http(&client, CONNECTIVITY_TARGETS[0], reqwest::Method::HEAD),
        probe_http(&client, CONNECTIVITY_TARGETS[1], reqwest::Method::HEAD),
        probe_http(&client, CONNECTIVITY_TARGETS[2], reqwest::Method::HEAD),
    );
    vec![a, b, c]
}

fn eligible_local_address(ip: &IpAddr) -> bool {
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
        return false;
    }
    match ip {
        IpAddr::V4(ip) => !ip.is_link_local() && !ip.is_broadcast(),
        IpAddr::V6(ip) => !ip.is_unicast_link_local(),
    }
}

fn addresses_from_interfaces(
    interfaces: Vec<crate::platform::NetworkInterfaceInfo>,
) -> Vec<LocalAddress> {
    let mut result: Vec<LocalAddress> = Vec::new();
    for interface in interfaces {
        if interface.is_loopback || !interface.is_up {
            continue;
        }
        for ip in interface
            .ipv4_addresses
            .into_iter()
            .chain(interface.ipv6_addresses)
        {
            if !eligible_local_address(&ip) {
                continue;
            }
            let address = ip.to_string();
            if result.iter().any(|item| {
                item.address == address && item.interface.as_deref() == Some(&interface.name)
            }) {
                continue;
            }
            result.push(LocalAddress {
                address,
                interface: Some(interface.name.clone()),
                family: if ip.is_ipv4() { "ipv4" } else { "ipv6" }.to_string(),
                source: "interface".to_string(),
            });
        }
    }
    result
}

/// Preserve all eligible candidates and their interfaces. UDP fallback yields
/// only an address selected for that route, and must not invent an interface.
fn get_local_addresses() -> Vec<LocalAddress> {
    let mut addresses =
        addresses_from_interfaces(crate::platform::get_network_interfaces().unwrap_or_default());
    for (family, bind, target) in [
        ("ipv4", "0.0.0.0:0", "8.8.8.8:53"),
        ("ipv6", "[::]:0", "[2001:4860:4860::8888]:53"),
    ] {
        if addresses.iter().any(|address| address.family == family) {
            continue;
        }
        if let Ok(socket) = std::net::UdpSocket::bind(bind) {
            if socket.connect(target).is_ok() {
                if let Ok(address) = socket.local_addr() {
                    if eligible_local_address(&address.ip()) {
                        addresses.push(LocalAddress {
                            address: address.ip().to_string(),
                            interface: None,
                            family: family.to_string(),
                            source: "route_fallback".to_string(),
                        });
                    }
                }
            }
        }
    }
    addresses
}

#[derive(Clone)]
struct PublicIpObservation {
    ip: Option<String>,
    probe: PublicIpProbe,
}

struct CachedPublicIp {
    observation: PublicIpObservation,
    fetched_at: Instant,
}

/// One process-wide entry, with failure caching and in-flight coalescing. Holding
/// an async lock during the bounded fetch makes concurrent misses share a result.
async fn cached_public_ip<F, Fut>(
    cache: &Mutex<Option<CachedPublicIp>>,
    fetch: F,
) -> PublicIpObservation
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = PublicIpObservation>,
{
    let mut guard = cache.lock().await;
    if let Some(entry) = guard.as_ref() {
        let ttl = if entry.observation.ip.is_some() {
            PUBLIC_IP_SUCCESS_TTL
        } else {
            PUBLIC_IP_FAILURE_TTL
        };
        if entry.fetched_at.elapsed() < ttl {
            let mut observation = entry.observation.clone();
            observation.probe.cache_hit = true;
            return observation;
        }
    }
    let observation = fetch().await;
    *guard = Some(CachedPublicIp {
        observation: observation.clone(),
        fetched_at: Instant::now(),
    });
    observation
}

async fn get_public_ip_cached() -> PublicIpObservation {
    static CACHE: OnceLock<Mutex<Option<CachedPublicIp>>> = OnceLock::new();
    cached_public_ip(CACHE.get_or_init(|| Mutex::new(None)), get_public_ip).await
}

async fn get_public_ip() -> PublicIpObservation {
    let mut failure = PublicIpObservation {
        ip: None,
        probe: PublicIpProbe {
            status: "error".to_string(),
            checked_at: Some(chrono::Utc::now().timestamp_millis()),
            ..PublicIpProbe::default()
        },
    };
    let client = match direct_http_client(Duration::from_secs(5)) {
        Ok(client) => client,
        Err(error) => {
            failure.probe.error = Some(error.to_string());
            return failure;
        }
    };
    let mut errors = Vec::new();
    // Each request, including reading its response body, is bounded by 5s.
    for service in PUBLIC_IP_SERVICES {
        match fetch_public_ipv4_from_service(&client, service).await {
            Ok((ip, family)) => {
                return PublicIpObservation {
                    ip: Some(ip),
                    probe: PublicIpProbe {
                        status: "success".to_string(),
                        url: Some(service.to_string()),
                        error: None,
                        proxy_policy: "no_proxy".to_string(),
                        checked_at: Some(chrono::Utc::now().timestamp_millis()),
                        address_family: family,
                        cache_hit: false,
                    },
                }
            }
            Err(error) => errors.push(format!("{}: {}", service, error)),
        }
    }
    failure.probe.error = Some(errors.join("; "));
    failure.probe.checked_at = Some(chrono::Utc::now().timestamp_millis());
    failure
}

async fn fetch_public_ipv4_from_service(
    client: &reqwest::Client,
    service: &str,
) -> Result<(String, Option<String>), String> {
    let response = client
        .get(service)
        .send()
        .await
        .map_err(|error| error_chain(&error))?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    let family = response
        .remote_addr()
        .map(|address| if address.is_ipv4() { "ipv4" } else { "ipv6" }.to_string());
    let body = response.text().await.map_err(|error| error_chain(&error))?;
    let ip = body.trim();
    match ip.parse::<IpAddr>() {
        Ok(IpAddr::V4(address))
            if eligible_local_address(&IpAddr::V4(address)) && !address.is_private() =>
        {
            Ok((ip.to_string(), family))
        }
        _ => Err("Service did not return a public IPv4 address".to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn interface(name: &str, ips: &[&str], up: bool) -> crate::platform::NetworkInterfaceInfo {
        let ips: Vec<IpAddr> = ips.iter().map(|ip| ip.parse().unwrap()).collect();
        crate::platform::NetworkInterfaceInfo {
            name: name.to_string(),
            display_name: name.to_string(),
            ipv4_addresses: ips.iter().filter(|ip| ip.is_ipv4()).copied().collect(),
            ipv6_addresses: ips.iter().filter(|ip| ip.is_ipv6()).copied().collect(),
            is_up: up,
            is_loopback: false,
            gateway: None,
        }
    }

    #[test]
    fn local_candidates_preserve_interfaces_and_filter_non_unicast_addresses() {
        let addresses = addresses_from_interfaces(vec![
            interface(
                "en0",
                &[
                    "192.168.1.2",
                    "2001:db8::1",
                    "fe80::1",
                    "::",
                    "ff02::1",
                    "127.0.0.1",
                ],
                true,
            ),
            interface("utun0", &["fd00::1", "2001:db8::1"], true),
            interface("down", &["2001:db8::2"], false),
        ]);
        assert_eq!(addresses.len(), 4);
        assert_eq!(addresses[1].interface.as_deref(), Some("en0"));
        assert_eq!(addresses[2].interface.as_deref(), Some("utun0"));
        assert_eq!(addresses[2].source, "interface");
        assert_eq!(addresses[2].family, "ipv6");
        // The same address on another interface retains both origins.
        assert_eq!(addresses[3].address, "2001:db8::1");
    }

    fn result(url: &str, success: bool) -> HttpProbeResult {
        let mut result = failed_http_probe(url, "connect", "test failure".into());
        result.success = success;
        if success {
            result.status_code = Some(204);
            result.error = None;
            result.error_kind = None;
        }
        result
    }

    #[tokio::test]
    async fn ipv6_only_and_dual_stack_hosts_run_probes_but_no_address_skips() {
        for ips in [
            &["2001:db8::1"][..],
            &["192.168.1.2", "2001:db8::1"][..],
            &["192.168.1.2"][..],
        ] {
            let addresses = addresses_from_interfaces(vec![interface("en0", ips, true)]);
            let mut called = false;
            let status = network_status_for_addresses(&addresses, || {
                called = true;
                async { vec![result("https://example.com", true)] }
            })
            .await;
            assert!(called);
            assert_eq!(status.status, "normal");
            assert_eq!(status.probes.len(), 1);
        }
        let status = network_status_for_addresses(&[], || async {
            panic!("No-address host must not probe")
        })
        .await;
        assert_eq!(status.status, "abnormal");
        assert!(status.probes.is_empty());
        assert!(status.message.contains("本地 IPv4/IPv6"));
    }

    #[test]
    fn summary_retains_partial_failure_and_limits_its_claim_to_targets() {
        let status = summarize_probes(vec![
            result("a", false),
            result("b", true),
            result("c", false),
        ]);
        assert_eq!(status.status, "normal");
        assert_eq!(status.probes.len(), 3);
        assert!(status.message.contains("1/3"));
        assert!(status.message.contains("仅代表本次所测目标"));
        let failure = summarize_probes(vec![result("a", false)]);
        assert_eq!(failure.status, "abnormal");
        assert!(failure.message.contains("不能据此判断"));
        assert!(!failure.message.contains("路由器"));
    }

    fn observation(success: bool) -> PublicIpObservation {
        PublicIpObservation {
            ip: success.then(|| "203.0.113.1".to_string()),
            probe: PublicIpProbe {
                status: if success { "success" } else { "error" }.to_string(),
                checked_at: Some(42),
                ..PublicIpProbe::default()
            },
        }
    }

    #[tokio::test]
    async fn concurrent_public_ip_misses_coalesce_for_success_and_failure() {
        for success in [true, false] {
            let cache = Arc::new(Mutex::new(None));
            let calls = Arc::new(AtomicUsize::new(0));
            let mut tasks = Vec::new();
            for _ in 0..12 {
                let cache = Arc::clone(&cache);
                let calls = Arc::clone(&calls);
                tasks.push(tokio::spawn(async move {
                    cached_public_ip(&cache, || async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        observation(success)
                    })
                    .await
                }));
            }
            let mut fresh = 0;
            for task in tasks {
                let value = task.await.unwrap();
                assert_eq!(value.ip.is_some(), success);
                assert_eq!(value.probe.checked_at, Some(42));
                fresh += usize::from(!value.probe.cache_hit);
            }
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(fresh, 1);
        }
    }

    #[tokio::test]
    async fn public_ip_success_and_failure_are_cached_only_until_ttl() {
        for success in [true, false] {
            let cache = Mutex::new(Some(CachedPublicIp {
                observation: observation(success),
                fetched_at: Instant::now() - Duration::from_secs(1),
            }));
            let mut called = false;
            let value = cached_public_ip(&cache, || {
                called = true;
                async { observation(true) }
            })
            .await;
            assert!(!called);
            assert!(value.probe.cache_hit);
            cache.lock().await.as_mut().unwrap().fetched_at =
                Instant::now() - PUBLIC_IP_SUCCESS_TTL;
            let value = cached_public_ip(&cache, || async { observation(false) }).await;
            assert!(!value.probe.cache_hit);
            assert!(value.ip.is_none());
        }
    }

    #[tokio::test]
    async fn public_ip_requires_successful_status_and_valid_v4_body() {
        for (status, body, valid) in [
            (200, "203.0.113.7", true),
            (503, "203.0.113.7", false),
            (200, "2001:db8::1", false),
            (200, "192.168.1.1", false),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                let received = socket.read(&mut request).await.unwrap();
                assert!(received > 0, "HTTP request must arrive before responding");
                socket
                    .write_all(
                        format!(
                            "HTTP/1.1 {} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            status,
                            body.len(),
                            body
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            });
            let result = fetch_public_ipv4_from_service(
                &direct_http_client(Duration::from_secs(1)).unwrap(),
                &url,
            )
            .await;
            assert_eq!(result.is_ok(), valid);
            if valid {
                assert_eq!(result.unwrap().1.as_deref(), Some("ipv4"));
            }
            server.await.unwrap();
        }
    }
    #[tokio::test]
    async fn local_only_mode_never_enters_external_probe_paths() {
        let addresses = addresses_from_interfaces(vec![interface(
            "en0",
            &["192.168.1.2", "2001:db8::1"],
            true,
        )]);
        let result = build_ip_info_with_probes(
            IpProbeMode::LocalOnly,
            addresses,
            || async { panic!("local sampling must not request public IP") },
            |_| async { panic!("local sampling must not request GeoIP") },
        )
        .await
        .unwrap();
        assert_eq!(result.local_ipv4.as_deref(), Some("192.168.1.2"));
        assert_eq!(result.local_ipv6.as_deref(), Some("2001:db8::1"));
        assert_eq!(result.ipv6_interface.as_deref(), Some("en0"));
        assert!(result.dual_stack_enabled);
        assert!(result.ipv4.is_none());
        assert_eq!(result.public_ipv4_probe.status, "skipped");
        assert!(result.public_ipv4_probe.checked_at.is_none());
        assert!(result.ipv4_geoip.is_none() && result.ipv6_geoip.is_none());
    }

    #[tokio::test]
    async fn explicit_external_mode_preserves_public_ip_and_geoip_opt_in_behavior() {
        for include_geoip in [false, true] {
            let addresses = addresses_from_interfaces(vec![interface(
                "en0",
                &["192.168.1.2", "2001:db8::1"],
                true,
            )]);
            let trace = Arc::new(std::sync::Mutex::new(Vec::new()));
            let public_trace = Arc::clone(&trace);
            let geo_trace = Arc::clone(&trace);
            let result = build_ip_info_with_probes(
                IpProbeMode::External { include_geoip },
                addresses,
                move || async move {
                    public_trace.lock().unwrap().push("public-ip".to_string());
                    observation(true)
                },
                move |ip| {
                    geo_trace.lock().unwrap().push(format!("geoip:{ip}"));
                    std::future::ready(None)
                },
            )
            .await
            .unwrap();
            assert_eq!(result.ipv4.as_deref(), Some("203.0.113.1"));
            let calls = trace.lock().unwrap();
            if include_geoip {
                assert_eq!(
                    *calls,
                    vec!["public-ip", "geoip:203.0.113.1", "geoip:2001:db8::1"]
                );
            } else {
                assert_eq!(*calls, vec!["public-ip"]);
            }
        }
    }
}
