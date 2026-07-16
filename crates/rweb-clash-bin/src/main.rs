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
    init_logging(cli.log_level.as_deref());

    let listen_addr = cli.listen_addr.unwrap_or_else(default_listen_addr);
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
    help: bool,
}

impl CliOptions {
    fn parse() -> anyhow::Result<Self> {
        let mut options = Self::default();
        let mut args = std::env::args().skip(1);
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

Environment:
  RWEB_CLASH_LISTEN   Listen address, default 127.0.0.1:31990
  RWEB_CLASH_ROOT     Runtime data directory
  RWEB_CLASH_LOG      Log level: trace, debug, info, warn, error
  RWEB_CLASH_API_TOKEN  Bearer token; required for non-loopback listen addresses
  RWEB_CLASH_MIHOMO_VALIDATION_TIMEOUT_SECS  Mihomo config validation timeout, default 120
"
    );
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
