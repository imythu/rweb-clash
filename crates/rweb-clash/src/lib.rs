mod api;
mod app;
mod assets;
mod backup;
mod bootstrap;
mod controller;
mod core;
mod egress;
mod error;
mod instance_lock;
mod manual;
mod paths;
mod platform;
mod proxy;
mod remote;
mod rule;
mod runtime;
mod storage;
mod subscription;
pub mod types;
mod util;

pub use app::{App, AppOptions};
pub use assets::{EmbeddedAssets, EmbeddedFile};
pub use error::{AppError, ErrorBody, ErrorEnvelope};
pub use paths::AppPaths;

pub fn router(app: App) -> axum::Router {
    api::router(app)
}

pub fn validate_api_access(addr: std::net::SocketAddr) -> anyhow::Result<()> {
    let token = std::env::var("RWEB_CLASH_API_TOKEN")
        .ok()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty());
    validate_api_access_with_token(addr, token.as_deref())
}

pub async fn bind_api_listener(
    addr: std::net::SocketAddr,
) -> anyhow::Result<tokio::net::TcpListener> {
    validate_api_access(addr)?;
    Ok(tokio::net::TcpListener::bind(addr).await?)
}

fn validate_api_access_with_token(
    addr: std::net::SocketAddr,
    token: Option<&str>,
) -> anyhow::Result<()> {
    if token.is_some_and(|token| token.len() < 16) {
        anyhow::bail!("RWEB_CLASH_API_TOKEN must contain at least 16 characters");
    }
    if !addr.ip().is_loopback() && token.is_none() {
        anyhow::bail!(
            "RWEB_CLASH_API_TOKEN is required when listening on non-loopback address {addr}"
        );
    }
    Ok(())
}

pub async fn serve_on_listener(app: App, listener: tokio::net::TcpListener) -> anyhow::Result<()> {
    tracing::info!("rweb-clash listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router(app)).await?;
    Ok(())
}

pub async fn serve_on_listener_with_shutdown<F>(
    app: App,
    listener: tokio::net::TcpListener,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tracing::info!("rweb-clash listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router(app))
        .with_graceful_shutdown(shutdown)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_listeners_require_a_strong_api_token() {
        let loopback = "127.0.0.1:31990".parse().unwrap();
        let remote = "0.0.0.0:31990".parse().unwrap();

        assert!(validate_api_access_with_token(loopback, None).is_ok());
        assert!(validate_api_access_with_token(remote, None).is_err());
        assert!(validate_api_access_with_token(remote, Some("too-short")).is_err());
        assert!(validate_api_access_with_token(remote, Some("0123456789abcdef")).is_ok());
    }

    #[tokio::test]
    async fn bound_listener_reserves_the_api_address() {
        let listener = bind_api_listener("127.0.0.1:0".parse().unwrap())
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();

        assert!(bind_api_listener(address).await.is_err());
    }
}
