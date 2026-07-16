use crate::types::EgressResponse;
use serde::Deserialize;
use std::time::Duration;
use tracing::{debug, warn};

const CLOUDFLARE_TRACE_URL: &str = "https://cloudflare.com/cdn-cgi/trace";

#[derive(Debug, Clone)]
pub struct EgressProbe {
    client: reqwest::Client,
}

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
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub async fn probe(&self) -> EgressResponse {
        debug!("probing egress network");
        let trace = self.cloudflare_trace().await.unwrap_or_default();
        let enrichment = match trace.ip.as_deref() {
            Some(ip) => self.ipapi(ip).await,
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
        };
        debug!(
            ip = ?response.ip,
            provider = ?response.provider,
            country = ?response.country,
            "egress probe completed"
        );
        response
    }

    async fn cloudflare_trace(&self) -> Option<CloudflareTrace> {
        let response = match self
            .client
            .get(CLOUDFLARE_TRACE_URL)
            .timeout(Duration::from_secs(5))
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                warn!(error = %err, "cloudflare trace request failed");
                return None;
            }
        };
        let response = match response.error_for_status() {
            Ok(response) => response,
            Err(err) => {
                warn!(error = %err, "cloudflare trace returned unexpected status");
                return None;
            }
        };
        let text = match response.text().await {
            Ok(text) => text,
            Err(err) => {
                warn!(error = %err, "cloudflare trace body read failed");
                return None;
            }
        };
        Some(parse_cloudflare_trace(&text))
    }

    async fn ipapi(&self, ip: &str) -> Option<IpApiResponse> {
        let response = match self
            .client
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

fn parse_cloudflare_trace(text: &str) -> CloudflareTrace {
    let mut trace = CloudflareTrace::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "ip" => trace.ip = Some(value.trim().to_string()),
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
}
