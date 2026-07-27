use crate::error::AppError;
use crate::types::DownloadRoute;
use axum::http::StatusCode;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, LOCATION, USER_AGENT};
use reqwest::{redirect::Policy, Url};
use serde::Deserialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use tracing::warn;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const DNS_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REDIRECTS: usize = 5;
const MAX_CONCURRENT_DOWNLOADS: usize = 4;
const DNS_CACHE_TTL: Duration = Duration::from_secs(300);
const MAX_DNS_CACHE_ENTRIES: usize = 1024;
const CLOUDFLARE_DOH_HOST: &str = "cloudflare-dns.com";
const CLOUDFLARE_DOH_URL: &str = "https://cloudflare-dns.com/dns-query";

static DOWNLOAD_PERMITS: Semaphore = Semaphore::const_new(MAX_CONCURRENT_DOWNLOADS);
type DnsCache = Mutex<HashMap<String, (Instant, Vec<IpAddr>)>>;
static DNS_VALIDATION_CACHE: OnceLock<DnsCache> = OnceLock::new();

#[derive(Debug)]
pub struct RemoteTextResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: String,
    pub route: String,
}

#[derive(Debug, Clone, Default)]
pub struct RouteOptions {
    pub core_proxy: Option<String>,
    pub system_proxy: Option<String>,
}

pub fn validate_http_url(value: &str) -> bool {
    parse_http_url(value).is_ok()
}

pub async fn get_text_routed(
    value: &str,
    user_agent: Option<&str>,
    max_bytes: usize,
    error_code: &'static str,
    route: DownloadRoute,
    options: RouteOptions,
) -> Result<RemoteTextResponse, AppError> {
    tokio::time::timeout(REQUEST_TIMEOUT, async {
        let _permit = DOWNLOAD_PERMITS.acquire().await.map_err(|_| {
            AppError::service_unavailable("download_unavailable", "download queue is unavailable")
        })?;
        let initial_url = parse_http_url(value)?;
        let candidates = route_candidates(route, options)?;
        let mut failures = Vec::new();
        for (route_name, proxy_url) in candidates {
            match get_text_inner(
                initial_url.clone(),
                user_agent,
                max_bytes,
                error_code,
                route_name,
                proxy_url.as_deref(),
            )
            .await
            {
                Ok(response) => return Ok(response),
                Err(error) if route == DownloadRoute::Auto => {
                    warn!(route = route_name, %error, "download route failed, trying fallback");
                    failures.push(format!("{route_name}: {}", error.message));
                }
                Err(error) => return Err(error),
            }
        }
        Err(AppError::new(
            StatusCode::BAD_GATEWAY,
            error_code,
            format!("all download routes failed: {}", failures.join("; ")),
        ))
    })
    .await
    .map_err(|_| {
        AppError::new(
            StatusCode::GATEWAY_TIMEOUT,
            error_code,
            "remote resource request timed out",
        )
    })?
}

fn route_candidates(
    route: DownloadRoute,
    options: RouteOptions,
) -> Result<Vec<(&'static str, Option<String>)>, AppError> {
    let unavailable = |name: &str| {
        AppError::service_unavailable(
            "download_route_unavailable",
            format!("the {name} download route is unavailable"),
        )
    };
    match route {
        DownloadRoute::Direct => Ok(vec![("direct", None)]),
        DownloadRoute::Core => Ok(vec![(
            "core",
            Some(options.core_proxy.ok_or_else(|| unavailable("core"))?),
        )]),
        DownloadRoute::System => Ok(vec![(
            "system",
            Some(
                options
                    .system_proxy
                    .ok_or_else(|| unavailable("system proxy"))?,
            ),
        )]),
        DownloadRoute::Auto => {
            let mut routes = vec![("direct", None)];
            if let Some(proxy) = options.core_proxy {
                routes.push(("core", Some(proxy)));
            }
            if let Some(proxy) = options.system_proxy {
                if !routes
                    .iter()
                    .any(|(_, existing)| existing.as_deref() == Some(proxy.as_str()))
                {
                    routes.push(("system", Some(proxy)));
                }
            }
            Ok(routes)
        }
    }
}

async fn get_text_inner(
    mut url: Url,
    user_agent: Option<&str>,
    max_bytes: usize,
    error_code: &'static str,
    route: &str,
    proxy_url: Option<&str>,
) -> Result<RemoteTextResponse, AppError> {
    for redirect_count in 0..=MAX_REDIRECTS {
        let resolved = resolve_public_addresses(&url).await?;
        let host = url
            .host_str()
            .ok_or_else(|| invalid_url("remote resource URL must include a host"))?;
        let mut client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .redirect(Policy::none());
        if let Some(proxy_url) = proxy_url {
            let proxy = reqwest::Proxy::all(proxy_url).map_err(|error| {
                AppError::bad_request(
                    "download_proxy_invalid",
                    format!("download proxy URL is invalid: {error}"),
                )
            })?;
            client = client.proxy(proxy);
        } else {
            client = client.no_proxy().resolve_to_addrs(host, &resolved);
        }
        let client = client.build().map_err(AppError::internal)?;

        let mut request = client.get(url.clone());
        if let Some(user_agent) = user_agent {
            request = request.header(USER_AGENT, user_agent);
        }
        let response = request
            .send()
            .await
            .map_err(|err| upstream_error(error_code, err))?;
        let status = response.status();

        if status.is_redirection() {
            if redirect_count == MAX_REDIRECTS {
                return Err(AppError::new(
                    StatusCode::BAD_GATEWAY,
                    error_code,
                    format!("remote resource exceeded {MAX_REDIRECTS} redirects"),
                ));
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| {
                    AppError::new(
                        StatusCode::BAD_GATEWAY,
                        error_code,
                        "remote redirect did not include a valid Location header",
                    )
                })?;
            url = resolve_redirect_url(&url, location, error_code)?;
            continue;
        }

        if !status.is_success() {
            return Err(AppError::new(
                StatusCode::BAD_GATEWAY,
                error_code,
                format!("remote resource returned {status}"),
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > max_bytes as u64)
        {
            return Err(payload_too_large(max_bytes));
        }

        let headers = response.headers().clone();
        let mut stream = response.bytes_stream();
        let mut body = Vec::with_capacity(max_bytes.min(64 * 1024));
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|err| upstream_error(error_code, err))?;
            if body.len().saturating_add(chunk.len()) > max_bytes {
                return Err(payload_too_large(max_bytes));
            }
            body.extend_from_slice(&chunk);
        }
        let body = String::from_utf8(body).map_err(|err| {
            AppError::new(
                StatusCode::BAD_GATEWAY,
                error_code,
                format!("remote resource is not valid UTF-8: {err}"),
            )
        })?;
        return Ok(RemoteTextResponse {
            status,
            headers,
            body,
            route: route.to_string(),
        });
    }

    unreachable!("redirect loop always returns or continues within the configured bound")
}

fn resolve_redirect_url(
    current: &Url,
    location: &str,
    error_code: &'static str,
) -> Result<Url, AppError> {
    let next = current.join(location).map_err(|err| {
        AppError::new(
            StatusCode::BAD_GATEWAY,
            error_code,
            format!("remote redirect URL is invalid: {err}"),
        )
    })?;
    if current.scheme() == "https" && next.scheme() != "https" {
        return Err(AppError::new(
            StatusCode::BAD_GATEWAY,
            error_code,
            "remote redirect must not downgrade HTTPS to HTTP",
        ));
    }
    parse_http_url(next.as_str())?;
    Ok(next)
}

fn parse_http_url(value: &str) -> Result<Url, AppError> {
    let url = Url::parse(value.trim())
        .map_err(|err| invalid_url(format!("remote resource URL is invalid: {err}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid_url("remote resource URL must use http or https"));
    }
    if url.host_str().is_none() {
        return Err(invalid_url("remote resource URL must include a host"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(invalid_url(
            "remote resource URL must not include embedded credentials",
        ));
    }
    Ok(url)
}

async fn resolve_public_addresses(url: &Url) -> Result<Vec<SocketAddr>, AppError> {
    let host = url
        .host_str()
        .ok_or_else(|| invalid_url("remote resource URL must include a host"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| invalid_url("remote resource URL has no usable port"))?;
    let allow_private = allow_private_sources();

    if !allow_private && (host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost")) {
        return Err(private_address_error(host));
    }

    let literal_ip = host.parse::<IpAddr>().ok();
    let addresses = if let Some(ip) = literal_ip {
        vec![SocketAddr::new(ip, port)]
    } else {
        tokio::time::timeout(DNS_TIMEOUT, tokio::net::lookup_host((host, port)))
            .await
            .map_err(|_| {
                AppError::new(
                    StatusCode::GATEWAY_TIMEOUT,
                    "remote_dns_timeout",
                    format!("DNS lookup for {host} timed out"),
                )
            })?
            .map_err(|err| {
                AppError::new(
                    StatusCode::BAD_GATEWAY,
                    "remote_dns_failed",
                    format!("DNS lookup for {host} failed: {err}"),
                )
            })?
            .collect::<Vec<_>>()
    };

    let mut seen = HashSet::new();
    let mut addresses = addresses
        .into_iter()
        .filter(|address| seen.insert(*address))
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(AppError::new(
            StatusCode::BAD_GATEWAY,
            "remote_dns_failed",
            format!("DNS lookup for {host} returned no addresses"),
        ));
    }
    if !allow_private {
        if literal_ip.is_none() && addresses.iter().any(|address| is_fake_ipv4(address.ip())) {
            addresses = resolve_with_public_doh(host, port).await?;
        }
        if addresses.iter().any(|address| !is_public_ip(address.ip())) {
            return Err(private_address_error(host));
        }
    }
    Ok(addresses)
}

async fn resolve_with_public_doh(host: &str, port: u16) -> Result<Vec<SocketAddr>, AppError> {
    let cache = DNS_VALIDATION_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(addresses) = {
        let mut cache = cache.lock().await;
        prune_dns_cache(&mut cache, Instant::now());
        cache.get(host).map(|(_, addresses)| addresses.clone())
    } {
        return Ok(addresses
            .into_iter()
            .map(|address| SocketAddr::new(address, port))
            .collect());
    }

    let doh_addresses = [
        SocketAddr::from((Ipv4Addr::new(1, 1, 1, 1), 443)),
        SocketAddr::from((Ipv4Addr::new(1, 0, 0, 1), 443)),
    ];
    let client = reqwest::Client::builder()
        .connect_timeout(DNS_TIMEOUT)
        .timeout(DNS_TIMEOUT)
        .redirect(Policy::none())
        .no_proxy()
        .resolve_to_addrs(CLOUDFLARE_DOH_HOST, &doh_addresses)
        .build()
        .map_err(AppError::internal)?;

    let mut addresses = Vec::new();
    let mut last_error = None;
    for record_type in ["A", "AAAA"] {
        match query_doh(&client, host, record_type).await {
            Ok(records) => addresses.extend(records),
            Err(error) => last_error = Some(error),
        }
    }
    addresses.sort_unstable();
    addresses.dedup();
    if addresses.is_empty() {
        return Err(last_error.unwrap_or_else(|| {
            AppError::new(
                StatusCode::BAD_GATEWAY,
                "remote_dns_validation_failed",
                format!("public DNS validation for {host} returned no addresses"),
            )
        }));
    }
    if addresses.iter().any(|address| !is_public_ip(*address)) {
        return Err(private_address_error(host));
    }

    let mut cache = cache.lock().await;
    prune_dns_cache(&mut cache, Instant::now());
    if cache.len() >= MAX_DNS_CACHE_ENTRIES {
        if let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, (cached_at, _))| *cached_at)
            .map(|(host, _)| host.clone())
        {
            cache.remove(&oldest);
        }
    }
    cache.insert(host.to_string(), (Instant::now(), addresses.clone()));
    Ok(addresses
        .into_iter()
        .map(|address| SocketAddr::new(address, port))
        .collect())
}

fn prune_dns_cache(cache: &mut HashMap<String, (Instant, Vec<IpAddr>)>, now: Instant) {
    cache.retain(|_, (cached_at, _)| now.saturating_duration_since(*cached_at) < DNS_CACHE_TTL);
}

async fn query_doh(
    client: &reqwest::Client,
    host: &str,
    record_type: &str,
) -> Result<Vec<IpAddr>, AppError> {
    let mut url = Url::parse(CLOUDFLARE_DOH_URL).map_err(AppError::internal)?;
    url.query_pairs_mut()
        .append_pair("name", host)
        .append_pair("type", record_type);
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/dns-json")
        .send()
        .await
        .map_err(|error| upstream_error("remote_dns_validation_failed", error))?
        .error_for_status()
        .map_err(|error| upstream_error("remote_dns_validation_failed", error))?;
    let response = response
        .json::<DohResponse>()
        .await
        .map_err(|error| upstream_error("remote_dns_validation_failed", error))?;
    if response.status != 0 {
        return Err(AppError::new(
            StatusCode::BAD_GATEWAY,
            "remote_dns_validation_failed",
            format!(
                "public DNS validation for {host} returned status {}",
                response.status
            ),
        ));
    }
    Ok(response
        .answers
        .into_iter()
        .filter(|answer| matches!(answer.record_type, 1 | 28))
        .filter_map(|answer| answer.data.parse().ok())
        .collect())
}

#[derive(Debug, Deserialize)]
struct DohResponse {
    #[serde(rename = "Status")]
    status: u32,
    #[serde(rename = "Answer", default)]
    answers: Vec<DohAnswer>,
}

#[derive(Debug, Deserialize)]
struct DohAnswer {
    #[serde(rename = "type")]
    record_type: u16,
    data: String,
}

fn allow_private_sources() -> bool {
    std::env::var("RWEB_CLASH_ALLOW_PRIVATE_SOURCES")
        .ok()
        .is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    }
}

fn is_fake_ipv4(ip: IpAddr) -> bool {
    let IpAddr::V4(ip) = ip else {
        return false;
    };
    let octets = ip.octets();
    octets[0] == 198 && matches!(octets[1], 18 | 19)
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 198 && matches!(octets[1], 18 | 19))
        || octets[0] >= 240)
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(ipv4) = ip.to_ipv4_mapped() {
        return is_public_ipv4(ipv4);
    }
    let segments = ip.segments();
    if segments[0] == 0x2002 {
        let embedded = Ipv4Addr::new(
            (segments[1] >> 8) as u8,
            segments[1] as u8,
            (segments[2] >> 8) as u8,
            segments[2] as u8,
        );
        return is_public_ipv4(embedded);
    }
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || segments[0] & 0xffc0 == 0xfec0
        || (segments[0] == 0x0064 && segments[1] == 0xff9b)
        || (segments[0] == 0x2001 && matches!(segments[1], 0x0000 | 0x0db8)))
}

fn invalid_url(message: impl Into<String>) -> AppError {
    AppError::bad_request("remote_url_invalid", message)
}

fn private_address_error(host: &str) -> AppError {
    AppError::bad_request(
        "remote_private_address_blocked",
        format!(
            "remote resource host {host} resolves to a private or reserved address; set RWEB_CLASH_ALLOW_PRIVATE_SOURCES=1 only for trusted local sources"
        ),
    )
}

fn payload_too_large(max_bytes: usize) -> AppError {
    AppError::new(
        StatusCode::PAYLOAD_TOO_LARGE,
        "remote_payload_too_large",
        format!("remote resource exceeds the {max_bytes} byte limit"),
    )
}

fn upstream_error(code: &'static str, error: reqwest::Error) -> AppError {
    let status = if error.is_timeout() {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::BAD_GATEWAY
    };
    AppError::new(status, code, error.without_url().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_structured_http_urls() {
        assert!(validate_http_url("https://example.com/profile.yaml"));
        assert!(!validate_http_url("https://"));
        assert!(!validate_http_url("ftp://example.com/profile.yaml"));
        assert!(!validate_http_url(
            "https://user:secret@example.com/profile.yaml"
        ));
    }

    #[test]
    fn rejects_https_redirect_downgrades() {
        let current = Url::parse("https://example.com/profile.yaml").unwrap();
        let error = resolve_redirect_url(
            &current,
            "http://cdn.example.com/profile.yaml",
            "test_remote_failed",
        )
        .unwrap_err();

        assert_eq!(error.status, StatusCode::BAD_GATEWAY);
        assert!(error.message.contains("must not downgrade"));
    }

    #[test]
    fn dns_cache_pruning_removes_only_expired_entries() {
        let now = Instant::now();
        let mut cache = HashMap::from([
            (
                "expired.example".into(),
                (now - DNS_CACHE_TTL - Duration::from_secs(1), vec![]),
            ),
            (
                "fresh.example".into(),
                (now - Duration::from_secs(1), vec![]),
            ),
        ]);

        prune_dns_cache(&mut cache, now);

        assert!(!cache.contains_key("expired.example"));
        assert!(cache.contains_key("fresh.example"));
    }

    #[test]
    fn rejects_non_public_ip_ranges() {
        for value in [
            "0.0.0.0",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "192.168.0.1",
            "198.18.0.1",
            "224.0.0.1",
        ] {
            assert!(!is_public_ip(value.parse().unwrap()), "{value}");
        }
        assert!(is_public_ip("1.1.1.1".parse().unwrap()));
        assert!(is_public_ip("2606:4700:4700::1111".parse().unwrap()));
        assert!(!is_public_ip("::1".parse().unwrap()));
        assert!(!is_public_ip("fc00::1".parse().unwrap()));
        assert!(!is_public_ip("2002:0a00:0001::1".parse().unwrap()));
        assert!(!is_public_ip("64:ff9b::10.0.0.1".parse().unwrap()));
    }
}
