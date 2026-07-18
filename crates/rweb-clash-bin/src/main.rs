use rweb_clash::{App, AppOptions};
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::Level;

#[cfg(feature = "embedded-assets")]
include!(concat!(env!("OUT_DIR"), "/embedded_assets.rs"));

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = CliOptions::parse()?;
    if cli.help {
        print_help();
        return Ok(());
    }

    let listen_addr = cli.listen_addr.unwrap_or_else(default_listen_addr);
    if let Some(wait) = cli.wait {
        wait_for_api(
            readiness_address(listen_addr),
            std::time::Duration::from_secs(wait.seconds),
            wait.require_ready_core,
        )
        .await?;
        return Ok(());
    }
    init_logging(cli.log_level.as_deref());
    let listener = rweb_clash::bind_api_listener(listen_addr).await?;

    let app = App::initialize(AppOptions {
        listen_addr,
        root_dir: runtime_root_dir(cli.data_dir),
        packaged_resources: None,
        embedded_assets: embedded_assets(),
    })
    .await?;
    let cleanup_app = app.clone();
    let server_result = rweb_clash::serve_on_listener_with_shutdown(app, listener, async move {
        shutdown_signal().await;
        tracing::info!("shutdown signal received");
    })
    .await;
    shutdown_until_clean(&cleanup_app).await;
    server_result
}

async fn shutdown_until_clean(app: &App) {
    loop {
        match app.shutdown().await {
            Ok(()) => break,
            Err(err) => {
                tracing::warn!("shutdown cleanup failed, retrying: {err}");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
}

#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};

    match signal(SignalKind::terminate()) {
        Ok(mut terminate) => {
            tokio::select! {
                result = tokio::signal::ctrl_c() => {
                    if let Err(error) = result {
                        tracing::warn!(%error, "failed waiting for Ctrl+C");
                    }
                }
                _ = terminate.recv() => {}
            }
        }
        Err(error) => {
            tracing::warn!(%error, "failed installing SIGTERM handler");
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::warn!(%error, "failed waiting for Ctrl+C");
            }
        }
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::warn!(%error, "failed waiting for Ctrl+C");
    }
}

#[derive(Debug, Default)]
struct CliOptions {
    listen_addr: Option<SocketAddr>,
    data_dir: Option<PathBuf>,
    log_level: Option<String>,
    wait: Option<WaitOptions>,
    help: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WaitOptions {
    seconds: u64,
    require_ready_core: bool,
}

impl CliOptions {
    fn parse() -> anyhow::Result<Self> {
        Self::parse_from(std::env::args().skip(1))
    }

    fn parse_from(args: impl IntoIterator<Item = String>) -> anyhow::Result<Self> {
        let mut options = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--listen" => {
                    let value = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--listen requires an address"))?;
                    options.listen_addr = Some(value.parse()?);
                }
                "--data-dir" => {
                    let value = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("--data-dir requires a path"))?;
                    options.data_dir = Some(value.into());
                }
                "--log-level" => {
                    options.log_level = Some(
                        args.next()
                            .ok_or_else(|| anyhow::anyhow!("--log-level requires a value"))?,
                    );
                }
                "--wait-api" | "--wait-ready" => {
                    if options.wait.is_some() {
                        return Err(anyhow::anyhow!(
                            "--wait-api and --wait-ready are mutually exclusive"
                        ));
                    }
                    let value = args
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("{arg} requires seconds"))?;
                    let seconds = value
                        .parse::<u64>()
                        .map_err(|_| anyhow::anyhow!("{arg} must be an integer"))?;
                    if !(1..=3_600).contains(&seconds) {
                        return Err(anyhow::anyhow!("{arg} must be between 1 and 3600 seconds"));
                    }
                    options.wait = Some(WaitOptions {
                        seconds,
                        require_ready_core: arg == "--wait-ready",
                    });
                }
                "--no-open" => {
                    // Reserved for packaged launchers that open the browser automatically.
                }
                "--help" | "-h" => options.help = true,
                unknown => return Err(anyhow::anyhow!("unknown argument: {unknown}")),
            }
        }
        Ok(options)
    }
}

fn print_help() {
    println!(
        "\
rweb-clash

Usage:
  rweb-clash [--listen 127.0.0.1:31990] [--data-dir PATH] [--log-level info] [--no-open]
  rweb-clash [--listen 127.0.0.1:31990] --wait-api SECONDS
  rweb-clash [--listen 127.0.0.1:31990] --wait-ready SECONDS

Environment:
  RWEB_CLASH_LISTEN   Listen address, default 127.0.0.1:31990
  RWEB_CLASH_ROOT     Runtime data directory
  RWEB_CLASH_LOG      Log level: trace, debug, info, warn, error
  RWEB_CLASH_API_TOKEN  Bearer token; required for non-loopback listen addresses
  RWEB_CLASH_MIHOMO_VALIDATION_TIMEOUT_SECS  Mihomo config validation timeout, default 120
"
    );
}

fn readiness_address(listen_addr: SocketAddr) -> SocketAddr {
    match listen_addr {
        SocketAddr::V4(address) if address.ip().is_unspecified() => {
            SocketAddr::from(([127, 0, 0, 1], address.port()))
        }
        SocketAddr::V6(address) if address.ip().is_unspecified() => {
            SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], address.port()))
        }
        address => address,
    }
}

async fn wait_for_api(
    address: SocketAddr,
    timeout: std::time::Duration,
    require_ready_core: bool,
) -> anyhow::Result<()> {
    tokio::time::timeout(timeout, async {
        loop {
            if api_is_responding(address, require_ready_core).await? {
                break Ok::<(), anyhow::Error>(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("timed out waiting for rweb-clash API at http://{address}"))?
}

async fn api_is_responding(address: SocketAddr, require_ready_core: bool) -> anyhow::Result<bool> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let Ok(Ok(mut stream)) = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio::net::TcpStream::connect(address),
    )
    .await
    else {
        return Ok(false);
    };
    let token = std::env::var("RWEB_CLASH_API_TOKEN")
        .ok()
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control));
    let authorization = token
        .as_deref()
        .map(|value| format!("Authorization: Bearer {value}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "GET /api/system/status HTTP/1.1\r\nHost: {address}\r\n{authorization}Connection: close\r\n\r\n"
    );
    if !matches!(
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            stream.write_all(request.as_bytes())
        )
        .await,
        Ok(Ok(()))
    ) {
        return Ok(false);
    }
    const MAX_RESPONSE_BYTES: u64 = 64 * 1024;
    let mut response = Vec::new();
    let Ok(Ok(_)) = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        stream
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut response),
    )
    .await
    else {
        return Ok(false);
    };
    if response.len() > MAX_RESPONSE_BYTES as usize {
        return Ok(false);
    }
    readiness_response_is_healthy(&response, require_ready_core).map_err(anyhow::Error::msg)
}

fn readiness_response_is_healthy(
    response: &[u8],
    require_ready_core: bool,
) -> Result<bool, String> {
    let Some(header_end) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        return Ok(false);
    };
    let Ok(headers) = std::str::from_utf8(&response[..header_end]) else {
        return Ok(false);
    };
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok());
    if matches!(status, Some(401 | 403)) {
        return Err("rweb-clash API readiness authentication failed".into());
    }
    if status != Some(200) {
        return Ok(false);
    }
    let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&response[header_end + 4..])
    else {
        return Ok(false);
    };
    let auto_start = payload
        .get("config")
        .and_then(|config| config.get("auto_start"))
        .and_then(serde_json::Value::as_bool);
    let core_state = payload
        .get("core")
        .and_then(|core| core.get("state"))
        .and_then(serde_json::Value::as_str);
    if !require_ready_core {
        return Ok(auto_start.is_some() && core_state.is_some());
    }
    match auto_start {
        Some(true) => match core_state {
            Some("running") => Ok(true),
            Some("starting" | "stopping") => Ok(false),
            Some(state) => Err(format!(
                "core auto-start did not reach the running state: {state}"
            )),
            None => Ok(false),
        },
        Some(false) => Ok(true),
        None => Ok(false),
    }
}

fn default_listen_addr() -> SocketAddr {
    std::env::var("RWEB_CLASH_LISTEN")
        .ok()
        .and_then(|value| value.parse::<SocketAddr>().ok())
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 31990)))
}

fn runtime_root_dir(cli_data_dir: Option<PathBuf>) -> Option<PathBuf> {
    if cli_data_dir.is_some() {
        return cli_data_dir;
    }
    if let Some(root) = std::env::var_os("RWEB_CLASH_ROOT").filter(|value| !value.is_empty()) {
        return Some(root.into());
    }
    if cfg!(feature = "embedded-assets") {
        return Some(PathBuf::from("rweb-clash-data"));
    }
    None
}

#[cfg(feature = "embedded-assets")]
fn embedded_assets() -> Option<&'static rweb_clash::EmbeddedAssets> {
    Some(&EMBEDDED_ASSETS)
}

#[cfg(not(feature = "embedded-assets"))]
fn embedded_assets() -> Option<&'static rweb_clash::EmbeddedAssets> {
    None
}

fn init_logging(cli_level: Option<&str>) {
    let level = cli_level
        .map(str::to_string)
        .or_else(|| {
            std::env::var("RWEB_CLASH_LOG")
                .or_else(|_| std::env::var("RUST_LOG"))
                .ok()
        })
        .and_then(|value| parse_level(&value))
        .unwrap_or(Level::INFO);

    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(true)
        .compact()
        .init();
    tracing::info!(%level, "logging initialized");
}

fn parse_level(value: &str) -> Option<Level> {
    let first = value.split(',').next()?.trim();
    let level = first.rsplit('=').next().unwrap_or(first).trim();
    match level.to_ascii_lowercase().as_str() {
        "trace" => Some(Level::TRACE),
        "debug" => Some(Level::DEBUG),
        "info" => Some(Level::INFO),
        "warn" | "warning" => Some(Level::WARN),
        "error" => Some(Level::ERROR),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn parses_wait_ready_and_rejects_unsafe_ranges() {
        let parsed = CliOptions::parse_from(["--wait-ready".into(), "60".into()]).unwrap();
        assert_eq!(
            parsed.wait,
            Some(WaitOptions {
                seconds: 60,
                require_ready_core: true
            })
        );
        let parsed = CliOptions::parse_from(["--wait-api".into(), "10".into()]).unwrap();
        assert_eq!(
            parsed.wait,
            Some(WaitOptions {
                seconds: 10,
                require_ready_core: false
            })
        );

        for seconds in ["0", "3601", "invalid"] {
            assert!(CliOptions::parse_from(["--wait-ready".into(), seconds.into()]).is_err());
        }
        assert!(CliOptions::parse_from([
            "--wait-api".into(),
            "10".into(),
            "--wait-ready".into(),
            "10".into()
        ])
        .is_err());
    }

    #[test]
    fn readiness_uses_loopback_for_unspecified_listeners() {
        assert_eq!(
            readiness_address("0.0.0.0:31990".parse().unwrap()),
            "127.0.0.1:31990".parse().unwrap()
        );
        assert_eq!(
            readiness_address("[::]:31990".parse().unwrap()),
            "[::1]:31990".parse().unwrap()
        );
    }

    #[tokio::test]
    async fn readiness_accepts_a_stopped_core_when_auto_start_is_disabled() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 256];
            let read = stream.read(&mut request).await.unwrap();
            assert!(request[..read].starts_with(b"GET /api/system/status"));
            let body = br#"{"core":{"state":"not_running"},"config":{"auto_start":false}}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                std::str::from_utf8(body).unwrap()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        wait_for_api(address, std::time::Duration::from_secs(1), true)
            .await
            .unwrap();
        server.await.unwrap();
    }

    #[test]
    fn readiness_requires_a_running_core_when_auto_start_is_enabled() {
        let response = |auto_start: bool, state: &str| {
            let body = format!(
                r#"{{"core":{{"state":"{state}"}},"config":{{"auto_start":{auto_start}}}}}"#
            );
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
        };

        assert_eq!(
            readiness_response_is_healthy(response(false, "not_running").as_bytes(), true),
            Ok(true)
        );
        assert_eq!(
            readiness_response_is_healthy(response(true, "running").as_bytes(), true),
            Ok(true)
        );
        assert!(readiness_response_is_healthy(response(true, "error").as_bytes(), true).is_err());
        assert_eq!(
            readiness_response_is_healthy(response(true, "error").as_bytes(), false),
            Ok(true)
        );
        assert!(readiness_response_is_healthy(
            b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n",
            false
        )
        .is_err());
    }
}
