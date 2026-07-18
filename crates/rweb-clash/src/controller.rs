use crate::error::AppError;
use crate::types::{ConnectionResponse, DelayResponse, TrafficResponse};
use axum::http::{Method, StatusCode};
use futures_util::StreamExt;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
use tracing::{info, warn};

const TRAFFIC_SAMPLE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_TRAFFIC_SAMPLE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct ControllerClient {
    base_url: String,
    secret: Option<String>,
    client: reqwest::Client,
}

impl ControllerClient {
    pub fn new(addr: String, secret: Option<String>) -> Result<Self, AppError> {
        let base_url = if addr.starts_with("http://") || addr.starts_with("https://") {
            addr.trim_end_matches('/').to_string()
        } else {
            format!("http://{}", addr.trim_end_matches('/'))
        };
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(10))
            .no_proxy()
            .build()
            .map_err(AppError::internal)?;
        Ok(Self {
            base_url,
            secret,
            client,
        })
    }

    pub async fn reload_config(&self, path: &str) -> Result<(), AppError> {
        info!(path, "reloading mihomo config through controller");
        self.request_empty(
            Method::PUT,
            "/configs",
            Some(json!({
                "path": path,
                "payload": "",
            })),
        )
        .await
    }

    pub async fn select_proxy(&self, group: &str, name: &str) -> Result<(), AppError> {
        info!(group, name, "selecting proxy through controller");
        self.request_empty(
            Method::PUT,
            &format!("/proxies/{}", urlencoding::encode(group)),
            Some(json!({ "name": name })),
        )
        .await
    }

    pub async fn proxy_delay(
        &self,
        name: &str,
        test_url: &str,
        timeout_ms: u64,
    ) -> Result<DelayResponse, AppError> {
        info!(name, timeout_ms, "testing proxy delay through controller");
        let path = format!(
            "/proxies/{}/delay?timeout={}&url={}",
            urlencoding::encode(name),
            timeout_ms,
            urlencoding::encode(test_url)
        );
        let result: ControllerDelay = self.request_json(Method::GET, &path, None::<()>).await?;
        Ok(DelayResponse {
            name: name.to_string(),
            delay: result.delay.unwrap_or_default(),
        })
    }

    pub async fn group_delay(
        &self,
        name: &str,
        test_url: &str,
        timeout_ms: u64,
    ) -> Result<Vec<DelayResponse>, AppError> {
        info!(
            name,
            timeout_ms, "testing proxy group delay through controller"
        );
        let path = format!(
            "/group/{}/delay?timeout={}&url={}",
            urlencoding::encode(name),
            timeout_ms,
            urlencoding::encode(test_url)
        );
        let result: std::collections::HashMap<String, i64> =
            self.request_json(Method::GET, &path, None::<()>).await?;
        Ok(result
            .into_iter()
            .map(|(name, delay)| DelayResponse { name, delay })
            .collect())
    }

    pub async fn traffic_sample(&self) -> Result<TrafficResponse, AppError> {
        tokio::time::timeout(TRAFFIC_SAMPLE_TIMEOUT, async {
            let url = self.url("/traffic");
            let mut request = self.client.get(url);
            if let Some(secret) = self.secret.as_deref().filter(|secret| !secret.is_empty()) {
                request = request.bearer_auth(secret);
            }
            let response = request.send().await.map_err(AppError::from)?;
            let status = response.status();
            if !status.is_success() {
                return Err(AppError::new(
                    status,
                    "controller_unexpected_status",
                    format!("controller returned {status}"),
                ));
            }

            let mut stream = response.bytes_stream();
            let mut payload = Vec::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(AppError::from)?;
                append_traffic_chunk(&mut payload, &chunk)?;
                if let Some(sample) = parse_first_traffic_frame(&payload)? {
                    return Ok(sample);
                }
            }

            Err(AppError::new(
                StatusCode::BAD_GATEWAY,
                "controller_decode_failed",
                "controller traffic stream ended before a complete JSON frame",
            ))
        })
        .await
        .map_err(|_| {
            AppError::new(
                StatusCode::GATEWAY_TIMEOUT,
                "controller_timeout",
                "controller traffic stream timed out",
            )
        })?
    }

    pub async fn connections(&self) -> Result<Vec<ConnectionResponse>, AppError> {
        let payload: ControllerConnections = self
            .request_json(Method::GET, "/connections", None::<()>)
            .await?;
        Ok(payload
            .connections
            .into_iter()
            .map(|conn| ConnectionResponse {
                id: conn.id,
                domain: conn.metadata.host.or(conn.metadata.destination_ip),
                rule: conn.rule,
                policy: conn.chains.last().cloned(),
                speed: "实时".into(),
            })
            .collect())
    }

    pub async fn close_connection(&self, id: &str) -> Result<(), AppError> {
        info!(connection_id = %id, "closing controller connection");
        self.request_empty(
            Method::DELETE,
            &format!("/connections/{}", urlencoding::encode(id)),
            None::<()>,
        )
        .await
    }

    pub async fn flush_dns(&self) -> Result<(), AppError> {
        info!("flushing controller dns cache");
        self.request_empty(Method::POST, "/cache/fakeip/flush", None::<()>)
            .await
    }

    async fn request_json<T, B>(
        &self,
        method: Method,
        path: &str,
        body: Option<B>,
    ) -> Result<T, AppError>
    where
        T: DeserializeOwned,
        B: serde::Serialize,
    {
        let response = self.request(method, path, body).await?;
        let status = response.status();
        let text = response.text().await.map_err(AppError::from)?;
        if status.is_success() {
            serde_json::from_str(&text).map_err(|err| {
                warn!(status = status.as_u16(), body = %text, error = %err, "controller response decode failed");
                AppError::new(StatusCode::BAD_GATEWAY, "controller_decode_failed", err.to_string())
            })
        } else {
            Err(AppError::new(
                status,
                "controller_unexpected_status",
                if text.is_empty() {
                    format!("controller returned {status}")
                } else {
                    text
                },
            ))
        }
    }

    async fn request_empty<B>(
        &self,
        method: Method,
        path: &str,
        body: Option<B>,
    ) -> Result<(), AppError>
    where
        B: serde::Serialize,
    {
        let response = self.request(method, path, body).await?;
        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            Err(AppError::new(
                status,
                "controller_unexpected_status",
                if text.is_empty() {
                    format!("controller returned {status}")
                } else {
                    text
                },
            ))
        }
    }

    async fn request<B>(
        &self,
        method: Method,
        path: &str,
        body: Option<B>,
    ) -> Result<reqwest::Response, AppError>
    where
        B: serde::Serialize,
    {
        let mut request = self.client.request(method, self.url(path));
        if let Some(secret) = self.secret.as_deref().filter(|secret| !secret.is_empty()) {
            request = request.bearer_auth(secret);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        request.send().await.map_err(AppError::from)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

#[derive(Debug, Deserialize)]
struct ControllerDelay {
    #[serde(default)]
    delay: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ControllerConnections {
    connections: Vec<ControllerConnection>,
}

#[derive(Debug, Deserialize)]
struct ControllerConnection {
    id: String,
    metadata: ControllerMetadata,
    rule: Option<String>,
    #[serde(default)]
    chains: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ControllerMetadata {
    host: Option<String>,
    #[serde(rename = "destinationIP")]
    destination_ip: Option<String>,
}

fn append_traffic_chunk(payload: &mut Vec<u8>, chunk: &[u8]) -> Result<(), AppError> {
    if chunk.len() > MAX_TRAFFIC_SAMPLE_BYTES.saturating_sub(payload.len()) {
        return Err(AppError::new(
            StatusCode::BAD_GATEWAY,
            "controller_payload_too_large",
            format!("controller traffic stream exceeded the {MAX_TRAFFIC_SAMPLE_BYTES} byte limit"),
        ));
    }
    payload.extend_from_slice(chunk);
    Ok(())
}

fn parse_first_traffic_frame(payload: &[u8]) -> Result<Option<TrafficResponse>, AppError> {
    let mut frames = serde_json::Deserializer::from_slice(payload).into_iter::<TrafficResponse>();
    match frames.next() {
        Some(Ok(sample)) => Ok(Some(sample)),
        Some(Err(error)) if error.is_eof() => Ok(None),
        Some(Err(error)) => Err(AppError::new(
            StatusCode::BAD_GATEWAY,
            "controller_decode_failed",
            error.to_string(),
        )),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traffic_parser_accumulates_a_json_frame_across_chunks() {
        let mut payload = Vec::new();
        append_traffic_chunk(&mut payload, b" \r\n{\"up\":12").expect("append first traffic chunk");
        assert!(parse_first_traffic_frame(&payload)
            .expect("parse incomplete traffic frame")
            .is_none());

        append_traffic_chunk(&mut payload, b"3,\"down\":456}\n")
            .expect("append second traffic chunk");
        let sample = parse_first_traffic_frame(&payload)
            .expect("parse complete traffic frame")
            .expect("traffic frame");
        assert_eq!(sample.up, 123);
        assert_eq!(sample.down, 456);
    }

    #[test]
    fn traffic_parser_returns_the_first_of_multiple_frames() {
        let sample =
            parse_first_traffic_frame(b"\n\t{\"up\":1,\"down\":2}\n{\"up\":3,\"down\":4}\n")
                .expect("parse traffic frames")
                .expect("first traffic frame");

        assert_eq!(sample.up, 1);
        assert_eq!(sample.down, 2);
    }

    #[test]
    fn traffic_accumulator_rejects_oversized_frames() {
        let mut payload = vec![b' '; MAX_TRAFFIC_SAMPLE_BYTES];
        let error = append_traffic_chunk(&mut payload, b"x")
            .expect_err("traffic frame over the limit must fail");

        assert_eq!(error.status, StatusCode::BAD_GATEWAY);
        assert_eq!(error.code, "controller_payload_too_large");
    }
}
