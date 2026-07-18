use crate::error::AppError;
use crate::types::EgressResponse;
use axum::http::StatusCode;
use serde::Deserialize;
use std::time::Duration;
use tracing::{debug, warn};

const CLOUDFLARE_TRACE_URL: &str = "https://cloudflare.com/cdn-cgi/trace";

#[derive(Debug, Clone, Default)]
pub struct EgressProbe;

#[derive(Debug, Clone, Default)]
struct CloudflareTrace {
    ip: Option<String>,
    loc: Option<String>,
    colo: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IpApiResponse {
    org: Option<String>,
    country_name: Option<String>,
    country: Option<String>,
}

impl EgressProbe {
    pub fn new() -> Self {
        Self
    }

    pub async fn probe(&self, proxy_url: Option<&str>) -> Result<EgressResponse, AppError> {
        let client = build_client(proxy_url)?;
        debug!(proxy_url, "probing egress network");
        let trace = self.cloudflare_trace(&client).await?;
        let enrichment = match trace.ip.as_deref() {
            Some(ip) => self.ipapi(&client, ip).await,
            None => None,
        };
        let provider = enrichment
            .as_ref()
            .and_then(|value| value.org.clone())
            .or_else(|| trace.colo.map(|colo| format!("Cloudflare colo {colo}")));
        let country = enrichment
            .as_ref()
            .and_then(|value| value.country_name.clone())
            .or_else(|| enrichment.as_ref().and_then(|value| value.country.clone()))
            .or(trace.loc);

        let response = EgressResponse {
            ip: trace.ip,
            provider,
            country,
            source: Some("cloudflare.com".into()),
        };
        debug!(
            ip = ?response.ip,
            provider = ?response.provider,
            country = ?response.country,
            "egress probe completed"
        );
        Ok(response)
    }

    async fn cloudflare_trace(
        &self,
        client: &reqwest::Client,
    ) -> Result<CloudflareTrace, AppError> {
        let response = match client
            .get(CLOUDFLARE_TRACE_URL)
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                warn!(error = %err, "cloudflare trace request failed");
                return Err(egress_probe_error(err.to_string()));
            }
        };
        let response = match response.error_for_status() {
            Ok(response) => response,
            Err(err) => {
                warn!(error = %err, "cloudflare trace returned unexpected status");
                return Err(egress_probe_error(err.to_string()));
            }
        };
        let text = match response.text().await {
            Ok(text) => text,
            Err(err) => {
                warn!(error = %err, "cloudflare trace body read failed");
                return Err(egress_probe_error(err.to_string()));
            }
        };
        let trace = parse_cloudflare_trace(&text);
        if trace.ip.is_none() {
            return Err(egress_probe_error(
                "cloudflare trace response did not contain a valid IP address",
            ));
        }
        Ok(trace)
    }

    async fn ipapi(&self, client: &reqwest::Client, ip: &str) -> Option<IpApiResponse> {
        let response = match client
            .get(format!("https://ipapi.co/{ip}/json/"))
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                warn!(error = %err, "ipapi request failed");
                return None;
            }
        };
        let response = match response.error_for_status() {
            Ok(response) => response,
            Err(err) => {
                warn!(error = %err, "ipapi returned unexpected status");
                return None;
            }
        };
        match response.json::<IpApiResponse>().await {
            Ok(payload) => Some(payload),
            Err(err) => {
                warn!(error = %err, "ipapi response decode failed");
                None
            }
        }
    }
}

fn build_client(proxy_url: Option<&str>) -> Result<reqwest::Client, AppError> {
    let mut builder = reqwest::Client::builder().no_proxy();
    if let Some(proxy_url) = proxy_url {
        let proxy = match reqwest::Proxy::all(proxy_url) {
            Ok(proxy) => proxy,
            Err(error) => {
                warn!(%error, proxy_url, "invalid egress proxy URL");
                return Err(egress_probe_error(error.to_string()));
            }
        };
        builder = builder.proxy(proxy);
    }
    match builder.build() {
        Ok(client) => Ok(client),
        Err(error) => {
            warn!(%error, proxy_url, "failed to build egress HTTP client");
            Err(egress_probe_error(error.to_string()))
        }
    }
}

fn egress_probe_error(message: impl Into<String>) -> AppError {
    AppError::new(
        StatusCode::BAD_GATEWAY,
        "egress_probe_failed",
        format!("failed to determine egress IP: {}", message.into()),
    )
}

fn parse_cloudflare_trace(text: &str) -> CloudflareTrace {
    let mut trace = CloudflareTrace::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "ip" => {
                let value = value.trim();
                if value.parse::<std::net::IpAddr>().is_ok() {
                    trace.ip = Some(value.to_string());
                }
            }
            "loc" => trace.loc = Some(value.trim().to_string()),
            "colo" => trace.colo = Some(value.trim().to_string()),
            _ => {}
        }
    }
    trace
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cloudflare_trace_fields() {
        let trace = parse_cloudflare_trace("fl=1\nip=203.0.113.1\nloc=HK\ncolo=HKG\n");

        assert_eq!(trace.ip.as_deref(), Some("203.0.113.1"));
        assert_eq!(trace.loc.as_deref(), Some("HK"));
        assert_eq!(trace.colo.as_deref(), Some("HKG"));
    }

    #[test]
    fn ignores_invalid_cloudflare_trace_ip() {
        let trace = parse_cloudflare_trace("ip=not-an-ip\nloc=HK\ncolo=HKG\n");

        assert_eq!(trace.ip, None);
        assert_eq!(trace.loc.as_deref(), Some("HK"));
        assert_eq!(trace.colo.as_deref(), Some("HKG"));
    }
}
