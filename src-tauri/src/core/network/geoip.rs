use crate::models::network::GeoIPStatus;
use crate::models::GeoIPInfo;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, OnceCell};

const CACHE_CAPACITY: usize = 256;
const SUCCESS_TTL: Duration = Duration::from_secs(30 * 60);
const NO_DATA_TTL: Duration = Duration::from_secs(5 * 60);
const FAILURE_TTL: Duration = Duration::from_secs(60);

#[derive(Clone, Copy)]
enum ProviderKind {
    IpapiCo,
    IpWhoIs,
}

struct Provider {
    kind: ProviderKind,
    base_url: String,
}

impl Provider {
    fn name(&self) -> &'static str {
        match self.kind {
            ProviderKind::IpapiCo => "ipapi.co",
            ProviderKind::IpWhoIs => "ipwho.is",
        }
    }

    fn url(&self, ip: IpAddr) -> String {
        let base = self.base_url.trim_end_matches('/');
        match self.kind {
            ProviderKind::IpapiCo => format!("{base}/{ip}/json/"),
            ProviderKind::IpWhoIs => format!("{base}/{ip}"),
        }
    }
}

#[derive(Clone)]
struct CachedResult {
    info: GeoIPInfo,
    expires_at: Instant,
    accessed_at: Instant,
}

#[derive(Default)]
struct LookupState {
    cache: HashMap<IpAddr, CachedResult>,
    // Weak entries disappear after cancelled callers drop their references;
    // cancellation cannot leave a permanently pending lookup behind.
    in_flight: HashMap<IpAddr, Weak<OnceCell<CachedResult>>>,
    rate_limited_until: HashMap<&'static str, Instant>,
}

struct GeoIpResolver {
    client: Result<reqwest::Client, String>,
    providers: Vec<Provider>,
    state: Mutex<LookupState>,
    now: Arc<dyn Fn() -> Instant + Send + Sync>,
}

impl GeoIpResolver {
    fn new() -> Self {
        Self {
            // This disables explicit/environment HTTP proxies, but OS routes,
            // VPNs and TUN devices can still determine the network path.
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(concat!("NetAssist/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|err| err.to_string()),
            providers: vec![
                Provider {
                    kind: ProviderKind::IpapiCo,
                    base_url: "https://ipapi.co".into(),
                },
                Provider {
                    kind: ProviderKind::IpWhoIs,
                    base_url: "https://ipwho.is".into(),
                },
            ],
            state: Mutex::new(LookupState::default()),
            now: Arc::new(Instant::now),
        }
    }

    async fn lookup(&self, ip: &str) -> GeoIPInfo {
        let addr = match ip.parse::<IpAddr>() {
            Ok(addr) => addr.to_canonical(),
            Err(_) => return failure(ip, None, "invalid_ip", "Invalid IP address"),
        };
        let ip = addr.to_string();
        // Include all IPv6 link-local addresses (fe80::/10), even on platforms
        // whose interface classifier is more restrictive.
        let local = crate::platform::common::is_local_ip(&addr)
            || matches!(addr, IpAddr::V6(v6) if v6.segments()[0] & 0xffc0 == 0xfe80);
        if local {
            return GeoIPInfo {
                country: "本地网络".into(),
                status: GeoIPStatus::Local,
                ip,
                queried_at: chrono::Utc::now().timestamp_millis(),
                ..GeoIPInfo::default()
            };
        }

        let pending = {
            let now = (self.now)();
            let mut state = self.state.lock().await;
            state.cache.retain(|_, entry| entry.expires_at > now);
            state.in_flight.retain(|_, entry| entry.strong_count() > 0);
            if let Some(entry) = state.cache.get_mut(&addr) {
                entry.accessed_at = now;
                return entry.info.clone();
            }
            match state.in_flight.get(&addr).and_then(Weak::upgrade) {
                Some(pending) => pending,
                None => {
                    let pending = Arc::new(OnceCell::new());
                    state.in_flight.insert(addr, Arc::downgrade(&pending));
                    pending
                }
            }
        };

        // OnceCell merges concurrent callers for the same canonical IP. If the
        // initializing caller is cancelled, another waiter can run the lookup.
        let entry = pending
            .get_or_init(|| async {
                let info = self.lookup_online(&ip).await;
                let now = (self.now)();
                CachedResult {
                    expires_at: now + ttl_for(&info.status),
                    accessed_at: now,
                    info,
                }
            })
            .await
            .clone();
        let mut state = self.state.lock().await;
        // A late waiter must not overwrite or remove a newer request.
        if state
            .in_flight
            .get(&addr)
            .is_some_and(|weak| weak.ptr_eq(&Arc::downgrade(&pending)))
        {
            if !state.cache.contains_key(&addr) && state.cache.len() >= CACHE_CAPACITY {
                if let Some(oldest) = state
                    .cache
                    .iter()
                    .min_by_key(|(_, cached)| cached.accessed_at)
                    .map(|(ip, _)| *ip)
                {
                    state.cache.remove(&oldest);
                }
            }
            state.cache.insert(addr, entry.clone());
            state.in_flight.remove(&addr);
        }
        entry.info
    }

    async fn lookup_online(&self, ip: &str) -> GeoIPInfo {
        let client = match &self.client {
            Ok(client) => client,
            Err(error) => return failure(ip, None, "client", error),
        };
        let addr: IpAddr = ip.parse().expect("lookup validates IP addresses");
        let mut last_error = failure(ip, None, "provider", "No GeoIP provider available");
        let mut no_data = None;
        let mut errors = Vec::new();
        for provider in &self.providers {
            let info = self.query_provider(client, provider, addr).await;
            match info.status {
                GeoIPStatus::Success => return info,
                GeoIPStatus::NoData => no_data = Some(info),
                _ => {
                    errors.push(format!(
                        "{} [{}]: {}",
                        provider.name(),
                        info.error_kind.as_deref().unwrap_or("provider"),
                        info.error.as_deref().unwrap_or("Lookup failed")
                    ));
                    last_error = info;
                }
            }
        }
        if let Some(info) = no_data {
            return info;
        }
        if !errors.is_empty() {
            last_error.error = Some(errors.join("; "));
        }
        last_error
    }

    async fn query_provider(
        &self,
        client: &reqwest::Client,
        provider: &Provider,
        addr: IpAddr,
    ) -> GeoIPInfo {
        let ip = addr.to_string();
        let name = Some(provider.name());
        if self
            .state
            .lock()
            .await
            .rate_limited_until
            .get(provider.name())
            .is_some_and(|until| *until > (self.now)())
        {
            return failure(
                &ip,
                name,
                "http_429",
                "Provider rate-limit backoff is active",
            );
        }
        let response = match client.get(provider.url(addr)).send().await {
            Ok(response) => response,
            Err(error) => {
                let kind = if error.is_timeout() {
                    "timeout"
                } else {
                    "network"
                };
                return failure(&ip, name, kind, &error_details(&error));
            }
        };
        let status = response.status();
        if !status.is_success() {
            let kind = if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let delay = retry_after(response.headers().get(reqwest::header::RETRY_AFTER));
                let deadline = (self.now)() + delay;
                self.state
                    .lock()
                    .await
                    .rate_limited_until
                    .entry(provider.name())
                    .and_modify(|until| *until = (*until).max(deadline))
                    .or_insert(deadline);
                "http_429"
            } else {
                "http_error"
            };
            return failure(&ip, name, kind, &format!("HTTP {status}"));
        }
        let data = match response.json::<serde_json::Value>().await {
            Ok(data) => data,
            Err(error) => {
                let kind = if error.is_timeout() {
                    "timeout"
                } else {
                    "parse"
                };
                return failure(&ip, name, kind, &error_details(&error));
            }
        };
        parse_response(provider, &ip, data)
    }
}

fn ttl_for(status: &GeoIPStatus) -> Duration {
    match status {
        GeoIPStatus::Success | GeoIPStatus::Local => SUCCESS_TTL,
        GeoIPStatus::NoData => NO_DATA_TTL,
        GeoIPStatus::Failed => FAILURE_TTL,
    }
}

fn retry_after(value: Option<&reqwest::header::HeaderValue>) -> Duration {
    let seconds = value
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value.parse::<u64>().ok().or_else(|| {
                chrono::DateTime::parse_from_rfc2822(value)
                    .ok()
                    .map(|date| (date.timestamp() - chrono::Utc::now().timestamp()).max(0) as u64)
            })
        })
        .unwrap_or(FAILURE_TTL.as_secs());
    Duration::from_secs(seconds.clamp(FAILURE_TTL.as_secs(), 24 * 60 * 60))
}

fn failure(ip: &str, provider: Option<&str>, kind: &str, message: &str) -> GeoIPInfo {
    GeoIPInfo {
        ip: ip.to_string(),
        provider: provider.map(str::to_string),
        queried_at: chrono::Utc::now().timestamp_millis(),
        error_kind: Some(kind.to_string()),
        error: Some(message.to_string()),
        ..GeoIPInfo::default()
    }
}

fn error_details(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

fn location_text(value: &serde_json::Value) -> Option<&str> {
    value.as_str().map(str::trim).filter(|text| {
        !text.is_empty() && *text != "-" && *text != "未知" && !text.eq_ignore_ascii_case("unknown")
    })
}

fn parse_response(provider: &Provider, ip: &str, data: serde_json::Value) -> GeoIPInfo {
    let name = Some(provider.name());
    if !data.is_object() {
        return failure(ip, name, "parse", "Expected a GeoIP JSON object");
    }
    let provider_error = match provider.kind {
        ProviderKind::IpapiCo => {
            data["error"].as_bool() == Some(true)
                || data["error"]
                    .as_str()
                    .is_some_and(|error| !error.is_empty())
        }
        ProviderKind::IpWhoIs => data["success"].as_bool() == Some(false),
    };
    if provider_error {
        let reason = location_text(&data["reason"])
            .or_else(|| location_text(&data["message"]))
            .or_else(|| location_text(&data["error"]))
            .unwrap_or("Provider rejected the lookup");
        return failure(ip, name, "provider", reason);
    }
    if let Some(returned_ip) = data.get("ip") {
        let returned = returned_ip
            .as_str()
            .and_then(|value| value.parse::<IpAddr>().ok())
            .map(|addr| addr.to_canonical());
        if returned != ip.parse::<IpAddr>().ok() {
            return failure(
                ip,
                name,
                "ip_mismatch",
                "Provider returned a different or invalid IP address",
            );
        }
    }
    let country = match provider.kind {
        ProviderKind::IpapiCo => {
            location_text(&data["country_name"]).or_else(|| location_text(&data["country"]))
        }
        ProviderKind::IpWhoIs => location_text(&data["country"]),
    };
    let Some(country) = country else {
        return GeoIPInfo {
            status: GeoIPStatus::NoData,
            ..failure(
                ip,
                name,
                "missing_country",
                "Provider returned no country information",
            )
        };
    };
    GeoIPInfo {
        country: country.to_string(),
        region: location_text(&data["region"])
            .or_else(|| location_text(&data["region_name"]))
            .unwrap_or("-")
            .to_string(),
        city: location_text(&data["city"]).unwrap_or("-").to_string(),
        latitude: data["latitude"].as_f64(),
        longitude: data["longitude"].as_f64(),
        status: GeoIPStatus::Success,
        ip: ip.to_string(),
        provider: name.map(str::to_string),
        queried_at: chrono::Utc::now().timestamp_millis(),
        error_kind: None,
        error: None,
    }
}

/// Look up a location independently of connectivity. The optional return type is
/// kept for existing callers; failures now carry evidence instead of vanishing.
pub async fn lookup_geoip(ip: &str) -> Option<GeoIPInfo> {
    static RESOLVER: OnceLock<GeoIpResolver> = OnceLock::new();
    Some(RESOLVER.get_or_init(GeoIpResolver::new).lookup(ip).await)
}

/// Get GeoIP information for multiple IPs.
pub async fn lookup_geoip_batch(ips: Vec<String>) -> HashMap<String, GeoIPInfo> {
    let mut results = HashMap::new();
    for ip in ips {
        if let Some(info) = lookup_geoip(&ip).await {
            results.insert(ip, info);
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::{JoinHandle, JoinSet};

    #[derive(Clone)]
    struct MockResponse {
        status: u16,
        body: &'static str,
        headers: &'static str,
        delay: Duration,
        gate: Option<Arc<tokio::sync::Semaphore>>,
    }

    impl MockResponse {
        fn json(body: &'static str) -> Self {
            Self {
                status: 200,
                body,
                headers: "",
                delay: Duration::ZERO,
                gate: None,
            }
        }
    }

    struct MockServer {
        url: String,
        requests: Arc<AtomicUsize>,
        paths: Arc<Mutex<Vec<String>>>,
        task: JoinHandle<()>,
    }

    impl MockServer {
        async fn start(responses: Vec<MockResponse>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let requests = Arc::new(AtomicUsize::new(0));
            let paths = Arc::new(Mutex::new(Vec::new()));
            let request_counter = requests.clone();
            let captured_paths = paths.clone();
            let task = tokio::spawn(async move {
                let mut connections = JoinSet::new();
                while let Ok((mut stream, _)) = listener.accept().await {
                    let index = request_counter.fetch_add(1, Ordering::SeqCst);
                    let response = responses[index.min(responses.len() - 1)].clone();
                    let paths = captured_paths.clone();
                    connections.spawn(async move {
                        let mut request = Vec::new();
                        loop {
                            let mut buffer = [0; 1024];
                            let size = stream.read(&mut buffer).await.unwrap_or(0);
                            if size == 0 {
                                return;
                            }
                            request.extend_from_slice(&buffer[..size]);
                            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                                break;
                            }
                        }
                        paths.lock().await.push(
                            String::from_utf8_lossy(&request)
                                .split_whitespace()
                                .nth(1)
                                .unwrap()
                                .to_string(),
                        );
                        if let Some(gate) = &response.gate {
                            gate.acquire().await.unwrap().forget();
                        }
                        tokio::time::sleep(response.delay).await;
                        let body = response.body;
                        let message = format!(
                            "HTTP/1.1 {} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n{}",
                            response.status, body.len(), response.headers, body
                        );
                        let _ = stream.write_all(message.as_bytes()).await;
                    });
                }
            });
            Self {
                url,
                requests,
                paths,
                task,
            }
        }

        fn count(&self) -> usize {
            self.requests.load(Ordering::SeqCst)
        }

        async fn wait_for_requests(&self, count: usize) {
            tokio::time::timeout(Duration::from_secs(2), async {
                while self.count() < count {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        }
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    fn resolver(server: &MockServer) -> (GeoIpResolver, Arc<AtomicU64>) {
        let mut resolver = GeoIpResolver::new();
        resolver.providers = vec![Provider {
            kind: ProviderKind::IpapiCo,
            base_url: server.url.clone(),
        }];
        let elapsed = Arc::new(AtomicU64::new(0));
        let clock = elapsed.clone();
        let start = Instant::now();
        resolver.now = Arc::new(move || start + Duration::from_secs(clock.load(Ordering::SeqCst)));
        (resolver, elapsed)
    }

    #[test]
    fn provider_urls_include_the_requested_ipv4_and_ipv6() {
        let resolver = GeoIpResolver::new();
        for ip in ["8.8.8.8", "2606:4700:4700::1111"] {
            let addr: IpAddr = ip.parse().unwrap();
            assert_eq!(
                resolver.providers[0].url(addr),
                format!("https://ipapi.co/{ip}/json/")
            );
            assert_eq!(
                resolver.providers[1].url(addr),
                format!("https://ipwho.is/{ip}")
            );
        }
    }

    #[test]
    fn provider_errors_missing_country_and_missing_city_are_distinct() {
        let resolver = GeoIpResolver::new();
        let provider = &resolver.providers[0];
        for error in [json!(true), json!("invalid request")] {
            let info = parse_response(
                provider,
                "8.8.8.8",
                json!({"error": error, "reason": "Denied"}),
            );
            assert_eq!(info.status, GeoIPStatus::Failed);
            assert_eq!(info.error_kind.as_deref(), Some("provider"));
            assert_eq!(info.error.as_deref(), Some("Denied"));
        }
        let absent_country = parse_response(provider, "8.8.8.8", json!({"country_name": null}));
        assert_eq!(absent_country.status, GeoIPStatus::NoData);
        assert_eq!(
            absent_country.error_kind.as_deref(),
            Some("missing_country")
        );
        let missing_city = parse_response(
            provider,
            "8.8.8.8",
            json!({"country_name": "United States"}),
        );
        assert_eq!(missing_city.status, GeoIPStatus::Success);
        assert_eq!(missing_city.city, "-");
        assert_eq!(missing_city.ip, "8.8.8.8");
        assert_eq!(missing_city.provider.as_deref(), Some("ipapi.co"));
        assert!(missing_city.queried_at > 0);
        assert!(missing_city.error.is_none());
        assert_eq!(
            serde_json::to_value(missing_city).unwrap()["status"],
            "success"
        );
        let denied = parse_response(
            &resolver.providers[1],
            "8.8.8.8",
            json!({"success": false, "message": "Reserved range"}),
        );
        assert_eq!(denied.status, GeoIPStatus::Failed);
        assert_eq!(denied.error.as_deref(), Some("Reserved range"));
    }

    #[test]
    fn provider_address_must_match_and_unknown_country_is_not_success() {
        let resolver = GeoIpResolver::new();
        let provider = &resolver.providers[0];
        let mismatch = parse_response(
            provider,
            "8.8.8.8",
            json!({"ip": "1.1.1.1", "country_name": "Australia"}),
        );
        assert_eq!(mismatch.status, GeoIPStatus::Failed);
        assert_eq!(mismatch.error_kind.as_deref(), Some("ip_mismatch"));
        assert_eq!(mismatch.ip, "8.8.8.8");
        let equivalent = parse_response(
            provider,
            "2606:4700:4700::1111",
            json!({"ip": "2606:4700:4700:0:0:0:0:1111", "country_name": "United States"}),
        );
        assert_eq!(equivalent.status, GeoIPStatus::Success);
        for country in ["", " ", "-", "未知", "Unknown", "unknown"] {
            let info = parse_response(provider, "8.8.8.8", json!({"country_name": country}));
            assert_eq!(info.status, GeoIPStatus::NoData);
        }
    }

    #[tokio::test]
    async fn redirects_are_reported_without_following_another_endpoint() {
        let primary = MockServer::start(vec![MockResponse {
            status: 302,
            // A fixed local URL makes even an accidental redirect harmless.
            headers: "Location: http://127.0.0.1:1/\r\n",
            ..MockResponse::json("{}")
        }])
        .await;
        let (resolver, _) = resolver(&primary);
        let info = resolver.lookup("8.8.8.8").await;
        assert_eq!(info.error_kind.as_deref(), Some("http_error"));
        assert!(info.error.unwrap().contains("302"));
        assert_eq!(primary.count(), 1);
    }

    #[tokio::test]
    async fn local_private_and_invalid_addresses_never_contact_a_provider() {
        let server = MockServer::start(vec![MockResponse::json("{}")]).await;
        let (resolver, _) = resolver(&server);
        for ip in [
            "192.168.1.2",
            "127.0.0.1",
            "fe80::1",
            "febf::1",
            "fd00::1",
            "::ffff:192.168.1.2",
        ] {
            assert_eq!(resolver.lookup(ip).await.status, GeoIPStatus::Local);
        }
        let invalid = resolver.lookup("invalid/ip?query").await;
        assert_eq!(invalid.error_kind.as_deref(), Some("invalid_ip"));
        assert_eq!(server.count(), 0);
    }

    #[tokio::test]
    async fn http_failure_parse_failure_and_timeout_are_preserved() {
        let cases = [
            (
                MockResponse {
                    status: 500,
                    ..MockResponse::json("{}")
                },
                "http_error",
            ),
            (MockResponse::json("not json"), "parse"),
            (MockResponse::json("[]"), "parse"),
            (
                MockResponse {
                    delay: Duration::from_millis(100),
                    ..MockResponse::json("{}")
                },
                "timeout",
            ),
        ];
        for (response, expected) in cases {
            let server = MockServer::start(vec![response]).await;
            let (mut resolver, _) = resolver(&server);
            resolver.client = Ok(reqwest::Client::builder()
                .no_proxy()
                .timeout(if expected == "timeout" {
                    Duration::from_millis(30)
                } else {
                    Duration::from_secs(3)
                })
                .build()
                .unwrap());
            let info = resolver.lookup("8.8.8.8").await;
            assert_eq!(info.status, GeoIPStatus::Failed);
            assert_eq!(info.error_kind.as_deref(), Some(expected));
            assert_eq!(info.ip, "8.8.8.8");
            assert_eq!(info.provider.as_deref(), Some("ipapi.co"));
            assert!(info.error.is_some());
        }
    }

    #[tokio::test]
    async fn fallback_uses_the_same_ip_and_reports_its_own_source() {
        let primary = MockServer::start(vec![MockResponse {
            status: 500,
            ..MockResponse::json("{}")
        }])
        .await;
        let fallback = MockServer::start(vec![MockResponse::json(
            r#"{"success":true,"country":"Australia"}"#,
        )])
        .await;
        let (mut resolver, _) = resolver(&primary);
        resolver.providers.push(Provider {
            kind: ProviderKind::IpWhoIs,
            base_url: fallback.url.clone(),
        });
        let result = resolver.lookup("2606:4700:4700::1111").await;
        assert_eq!(result.status, GeoIPStatus::Success);
        assert_eq!(result.provider.as_deref(), Some("ipwho.is"));
        assert_eq!(primary.paths.lock().await[0], "/2606:4700:4700::1111/json/");
        assert_eq!(fallback.paths.lock().await[0], "/2606:4700:4700::1111");
    }

    #[tokio::test]
    async fn success_no_data_and_failure_cache_expire_at_their_own_ttl() {
        for (body, status, ttl) in [
            (
                r#"{"country_name":"United States"}"#,
                GeoIPStatus::Success,
                SUCCESS_TTL,
            ),
            ("{}", GeoIPStatus::NoData, NO_DATA_TTL),
            ("not json", GeoIPStatus::Failed, FAILURE_TTL),
        ] {
            let server = MockServer::start(vec![MockResponse::json(body)]).await;
            let (resolver, clock) = resolver(&server);
            let first = resolver.lookup("8.8.8.8").await;
            assert_eq!(first.status, status);
            clock.store(ttl.as_secs() - 1, Ordering::SeqCst);
            let cached = resolver.lookup("8.8.8.8").await;
            assert_eq!(cached.queried_at, first.queried_at);
            assert_eq!(server.count(), 1);
            clock.store(ttl.as_secs(), Ordering::SeqCst);
            assert_eq!(resolver.lookup("8.8.8.8").await.status, status);
            assert_eq!(server.count(), 2);
        }
    }

    #[tokio::test]
    async fn concurrent_same_ip_requests_share_one_fetch() {
        let server = MockServer::start(vec![MockResponse {
            delay: Duration::from_millis(20),
            ..MockResponse::json(r#"{"country_name":"United States"}"#)
        }])
        .await;
        let (resolver, _) = resolver(&server);
        let resolver = Arc::new(resolver);
        let mut tasks = JoinSet::new();
        for _ in 0..20 {
            let resolver = resolver.clone();
            tasks.spawn(async move { resolver.lookup("8.8.8.8").await });
        }
        while let Some(result) = tasks.join_next().await {
            assert_eq!(result.unwrap().status, GeoIPStatus::Success);
        }
        assert_eq!(server.count(), 1);
    }

    #[tokio::test]
    async fn cancelled_initializer_does_not_leave_a_pending_lookup() {
        let server = MockServer::start(vec![
            MockResponse {
                delay: Duration::from_secs(10),
                ..MockResponse::json("{}")
            },
            MockResponse::json(r#"{"country_name":"United States"}"#),
        ])
        .await;
        let (resolver, _) = resolver(&server);
        let resolver = Arc::new(resolver);
        let first = resolver.clone();
        let task = tokio::spawn(async move { first.lookup("8.8.8.8").await });
        server.wait_for_requests(1).await;
        task.abort();
        let _ = task.await;
        let info = tokio::time::timeout(Duration::from_secs(2), resolver.lookup("8.8.8.8"))
            .await
            .unwrap();
        assert_eq!(info.status, GeoIPStatus::Success);
        assert_eq!(server.count(), 2);
    }

    #[tokio::test]
    async fn equivalent_ipv6_addresses_share_cache_but_different_ips_do_not() {
        let server = MockServer::start(vec![MockResponse::json(
            r#"{"country_name":"United States"}"#,
        )])
        .await;
        let (resolver, _) = resolver(&server);
        resolver
            .lookup("2606:4700:4700:0000:0000:0000:0000:1111")
            .await;
        let same = resolver.lookup("2606:4700:4700::1111").await;
        assert_eq!(same.ip, "2606:4700:4700::1111");
        assert_eq!(server.count(), 1);
        let different = resolver.lookup("1.1.1.1").await;
        assert_eq!(different.ip, "1.1.1.1");
        assert_eq!(server.count(), 2);
    }

    #[tokio::test]
    async fn rate_limit_backoff_applies_across_ips_without_reusing_locations() {
        let server = MockServer::start(vec![
            MockResponse {
                status: 429,
                headers: "Retry-After: 120\r\n",
                ..MockResponse::json("{}")
            },
            MockResponse::json(r#"{"country_name":"United States"}"#),
        ])
        .await;
        let (resolver, clock) = resolver(&server);
        let first = resolver.lookup("8.8.8.8").await;
        assert_eq!(first.error_kind.as_deref(), Some("http_429"));
        let second = resolver.lookup("1.1.1.1").await;
        assert_eq!(second.ip, "1.1.1.1");
        assert_eq!(second.error_kind.as_deref(), Some("http_429"));
        clock.store(61, Ordering::SeqCst);
        resolver.lookup("8.8.8.8").await;
        assert_eq!(server.count(), 1);
        clock.store(121, Ordering::SeqCst);
        assert_eq!(
            resolver.lookup("8.8.8.8").await.status,
            GeoIPStatus::Success
        );
        assert_eq!(server.count(), 2);
    }

    #[tokio::test]
    async fn concurrent_rate_limits_keep_the_longest_deadline_in_either_response_order() {
        for headers in [
            ["Retry-After: 3600\r\n", "Retry-After: 60\r\n"],
            ["Retry-After: 60\r\n", "Retry-After: 3600\r\n"],
        ] {
            let first_gate = Arc::new(tokio::sync::Semaphore::new(0));
            let second_gate = Arc::new(tokio::sync::Semaphore::new(0));
            let server = MockServer::start(vec![
                MockResponse {
                    status: 429,
                    headers: headers[0],
                    gate: Some(first_gate.clone()),
                    ..MockResponse::json("{}")
                },
                MockResponse {
                    status: 429,
                    headers: headers[1],
                    gate: Some(second_gate.clone()),
                    ..MockResponse::json("{}")
                },
                MockResponse::json(r#"{"country_name":"United States"}"#),
            ])
            .await;
            let (resolver, clock) = resolver(&server);
            let resolver = Arc::new(resolver);
            let first_resolver = resolver.clone();
            let first = tokio::spawn(async move { first_resolver.lookup("8.8.8.8").await });
            server.wait_for_requests(1).await;
            let second_resolver = resolver.clone();
            let second = tokio::spawn(async move { second_resolver.lookup("1.1.1.1").await });
            server.wait_for_requests(2).await;
            // Both requests passed the preflight check. Commit their 429
            // responses in a deterministic order, without timing-based sleeps.
            first_gate.add_permits(1);
            assert_eq!(first.await.unwrap().error_kind.as_deref(), Some("http_429"));
            second_gate.add_permits(1);
            assert_eq!(
                second.await.unwrap().error_kind.as_deref(),
                Some("http_429")
            );

            clock.store(61, Ordering::SeqCst);
            let during_backoff = resolver.lookup("8.8.4.4").await;
            assert_eq!(during_backoff.error_kind.as_deref(), Some("http_429"));
            assert_eq!(
                server.count(),
                2,
                "the shorter response must not shorten backoff"
            );
            clock.store(3600, Ordering::SeqCst);
            assert_eq!(
                resolver.lookup("9.9.9.9").await.status,
                GeoIPStatus::Success
            );
            assert_eq!(server.count(), 3);
        }
    }

    #[tokio::test]
    async fn cache_capacity_is_bounded_and_evicts_the_least_recently_used_ip() {
        let server = MockServer::start(vec![MockResponse::json(
            r#"{"country_name":"United States"}"#,
        )])
        .await;
        let (resolver, clock) = resolver(&server);
        for i in 0..CACHE_CAPACITY {
            clock.store(i as u64, Ordering::SeqCst);
            resolver
                .lookup(&format!("8.8.{}.{}", i / 256, i % 256))
                .await;
        }
        clock.store(CACHE_CAPACITY as u64, Ordering::SeqCst);
        resolver.lookup("8.8.0.0").await;
        resolver.lookup("8.8.1.0").await;
        let state = resolver.state.lock().await;
        assert_eq!(state.cache.len(), CACHE_CAPACITY);
        assert!(state.cache.contains_key(&"8.8.0.0".parse().unwrap()));
        assert!(!state.cache.contains_key(&"8.8.0.1".parse().unwrap()));
        assert!(state.cache.contains_key(&"8.8.1.0".parse().unwrap()));
        assert!(state.in_flight.is_empty());
    }

    #[test]
    fn retry_after_is_bounded_and_supports_http_dates() {
        use reqwest::header::HeaderValue;
        assert_eq!(retry_after(None), FAILURE_TTL);
        assert_eq!(
            retry_after(Some(&HeaderValue::from_static("120"))),
            Duration::from_secs(120)
        );
        assert_eq!(
            retry_after(Some(&HeaderValue::from_static("0"))),
            FAILURE_TTL
        );
        assert_eq!(
            retry_after(Some(&HeaderValue::from_static("999999"))),
            Duration::from_secs(86400)
        );
        let future = (chrono::Utc::now() + chrono::Duration::seconds(180)).to_rfc2822();
        let delay = retry_after(Some(&HeaderValue::from_str(&future).unwrap()));
        assert!((179..=180).contains(&delay.as_secs()));
    }
}
