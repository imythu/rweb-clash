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
    std::fs::set_permissions(SOCKET, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
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
                let mut guard = process.lock().await;
                if let Some(child) = guard.as_mut() {
                    let _ = child.kill().await;
                }
                *guard = None;
                Response {
                    ok: true,
                    error: None,
                    pid: None,
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
    for value in [binary, config, state_dir, client] {
        if !Path::new(value).is_absolute() {
            return Err("paths must be absolute".into());
        }
    }
    if !Path::new(binary).starts_with("/Library/Application Support/rweb-clash/") {
        return Err("binary is outside the managed installation directory".into());
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
