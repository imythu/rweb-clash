//! Privileged launchd helper for macOS TUN.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

const SOCKET: &str = "/var/run/rweb-clash-tun.sock";

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Ping,
    Stop,
    Start {
        binary: String,
        config: String,
        state_dir: String,
        binary_sha256: String,
        client_path: String,
    },
    Status,
}

#[derive(Debug, Serialize)]
struct Response {
    ok: bool,
    error: Option<String>,
    pid: Option<u32>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = std::fs::remove_file(SOCKET);
    let listener = UnixListener::bind(SOCKET).context("bind helper socket")?;
    // The desktop client runs as the logged-in user while this daemon runs as root.
    std::fs::set_permissions(SOCKET, std::os::unix::fs::PermissionsExt::from_mode(0o666))?;
    let process = Arc::new(Mutex::new(None::<Child>));
    loop {
        let (stream, _) = listener.accept().await?;
        let process = process.clone();
        tokio::spawn(async move {
            let _ = serve(stream, process).await;
        });
    }
}

async fn serve(stream: tokio::net::UnixStream, process: Arc<Mutex<Option<Child>>>) -> Result<()> {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    while let Some(line) = lines.next_line().await? {
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(Request::Ping) => Response {
                ok: true,
                error: None,
                pid: None,
            },
            Ok(Request::Stop) => {
                let child = process.lock().await.take();
                let result = if let Some(mut child) = child {
                    match child.start_kill() {
                        Ok(()) => child
                            .wait()
                            .await
                            .map(|_| ())
                            .map_err(|error| error.to_string()),
                        Err(error) => Err(error.to_string()),
                    }
                } else {
                    Ok(())
                };
                match result {
                    Ok(()) => Response {
                        ok: true,
                        error: None,
                        pid: None,
                    },
                    Err(error) => Response {
                        ok: false,
                        error: Some(format!("failed to stop Mihomo: {error}")),
                        pid: None,
                    },
                }
            }
            Ok(Request::Status) => {
                let mut guard = process.lock().await;
                let running = match guard.as_mut() {
                    Some(child) => match child.try_wait() {
                        Ok(Some(_)) => {
                            *guard = None;
                            false
                        }
                        _ => true,
                    },
                    None => false,
                };
                Response {
                    ok: running,
                    error: None,
                    pid: guard.as_ref().and_then(|c| c.id()),
                }
            }
            Ok(Request::Start {
                binary,
                config,
                state_dir,
                binary_sha256,
                client_path,
            }) => {
                if let Err(error) =
                    validate_request(&binary, &config, &state_dir, &binary_sha256, &client_path)
                        .await
                {
                    Response {
                        ok: false,
                        error: Some(error),
                        pid: None,
                    }
                } else {
                    let mut guard = process.lock().await;
                    if guard.is_some() {
                        Response {
                            ok: false,
                            error: Some("helper already has a running process".into()),
                            pid: None,
                        }
                    } else {
                        let log_path = Path::new(&state_dir).join("mihomo.log");
                        let log = match std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(log_path)
                        {
                            Ok(file) => file,
                            Err(error) => return Err(error.into()),
                        };
                        let err_log = log.try_clone()?;
                        match Command::new(&binary)
                            .args(["-d", &state_dir, "-f", &config])
                            .stdin(Stdio::null())
                            .stdout(Stdio::from(log))
                            .stderr(Stdio::from(err_log))
                            .spawn()
                        {
                            Ok(child) => {
                                let pid = child.id();
                                *guard = Some(child);
                                Response {
                                    ok: true,
                                    error: None,
                                    pid,
                                }
                            }
                            Err(error) => Response {
                                ok: false,
                                error: Some(error.to_string()),
                                pid: None,
                            },
                        }
                    }
                }
            }
            Err(error) => Response {
                ok: false,
                error: Some(error.to_string()),
                pid: None,
            },
        };
        write
            .write_all(serde_json::to_string(&response)?.as_bytes())
            .await?;
        write.write_all(b"\n").await?;
    }
    Ok(())
}

async fn validate_request(
    binary: &str,
    config: &str,
    state_dir: &str,
    expected: &str,
    client: &str,
) -> Result<(), String> {
    let binary_path = Path::new(binary);
    let config_path = Path::new(config);
    let state_dir_path = Path::new(state_dir);
    let client_path = Path::new(client);
    for path in [binary_path, config_path, state_dir_path, client_path] {
        if !path.is_absolute() {
            return Err("paths must be absolute".into());
        }
    }

    let app_root = managed_app_root(binary_path)
        .ok_or_else(|| "binary is outside the managed installation directory".to_string())?;
    if !binary_path.starts_with(app_root.join("cache-core"))
        || !config_path.starts_with(app_root.join("data/profiles"))
        || !state_dir_path.starts_with(app_root.join("data/profiles"))
    {
        return Err("TUN paths are outside the managed application data directories".into());
    }
    let bytes = tokio::fs::read(binary).await.map_err(|e| e.to_string())?;
    use sha2::{Digest, Sha256};
    if hex::encode(Sha256::digest(bytes)) != expected {
        return Err("mihomo hash verification failed".into());
    }
    let status = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict", client])
        .status()
        .await
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("client code signature verification failed".into());
    }
    Ok(())
}

fn managed_app_root(binary: &Path) -> Option<std::path::PathBuf> {
    let legacy_root = Path::new("/Library/Application Support/rweb-clash");
    if binary.starts_with(legacy_root) {
        return Some(legacy_root.to_path_buf());
    }

    let mut components = binary.components();
    if components.next()? != std::path::Component::RootDir {
        return None;
    }
    if components.next()?.as_os_str() != "Users" {
        return None;
    }
    let user = components.next()?.as_os_str();
    if user.is_empty() {
        return None;
    }
    let root = std::path::PathBuf::from("/Users")
        .join(user)
        .join("Library/Application Support/dev.rweb-clash.client");
    binary.starts_with(&root).then_some(root)
}

#[cfg(test)]
mod tests {
    use super::managed_app_root;
    use std::path::Path;

    #[test]
    fn accepts_the_desktop_app_support_root() {
        assert_eq!(
            managed_app_root(Path::new(
                "/Users/alice/Library/Application Support/dev.rweb-clash.client/cache-core/mihomo",
            ))
            .as_deref(),
            Some(Path::new(
                "/Users/alice/Library/Application Support/dev.rweb-clash.client",
            )),
        );
    }

    #[test]
    fn accepts_the_legacy_system_root() {
        assert_eq!(
            managed_app_root(Path::new(
                "/Library/Application Support/rweb-clash/cache-core/mihomo",
            ))
            .as_deref(),
            Some(Path::new("/Library/Application Support/rweb-clash")),
        );
    }

    #[test]
    fn rejects_other_application_data() {
        assert!(managed_app_root(Path::new(
            "/Users/alice/Library/Application Support/other-app/cache-core/mihomo",
        ))
        .is_none());
    }
}
