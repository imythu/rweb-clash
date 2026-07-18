#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rweb_clash::{App, AppOptions};
use std::fs::{File, OpenOptions, TryLockError};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use tokio::sync::oneshot;
use tracing::Level;

fn main() {
    init_logging();
    let instance_lock = match InstanceLock::acquire() {
        Ok(lock) => lock,
        Err(err) => {
            tracing::warn!("{err}");
            return;
        }
    };
    let backend_state = Arc::new(Mutex::new(BackendState::default()));
    let setup_state = backend_state.clone();
    let exit_state = backend_state.clone();
    let tray_state = backend_state.clone();
    tauri::Builder::default()
        .setup(move |app| {
            let app_data_dir = app.path().app_data_dir()?;
            if let Ok(mut state) = setup_state.lock() {
                state.instance_lock = Some(instance_lock);
            }
            install_tray(app, tray_state.clone())?;
            let resource_dir = packaged_resource_dir(app);
            let listen_addr = listen_addr();
            rweb_clash::validate_api_access(listen_addr)?;
            let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
            if let Ok(mut state) = setup_state.lock() {
                state.shutdown_tx = Some(shutdown_tx);
            }
            let state_for_thread = setup_state.clone();

            let worker = std::thread::spawn(move || {
                let runtime = match tokio::runtime::Runtime::new() {
                    Ok(runtime) => runtime,
                    Err(err) => {
                        tracing::error!("failed to create backend runtime: {err}");
                        return;
                    }
                };
                runtime.block_on(async move {
                    let listener = match rweb_clash::bind_api_listener(listen_addr).await {
                        Ok(listener) => listener,
                        Err(err) => {
                            tracing::error!(
                                "failed to bind backend API before initialization: {err}"
                            );
                            return;
                        }
                    };
                    let app = App::initialize(AppOptions {
                        root_dir: Some(app_data_dir),
                        packaged_resources: resource_dir,
                        embedded_assets: None,
                        listen_addr,
                    })
                    .await;
                    match app {
                        Ok(app) => {
                            let cleanup_app = app.clone();
                            if let Ok(mut state) = state_for_thread.lock() {
                                state.app = Some(app.clone());
                            }
                            let server_result = rweb_clash::serve_on_listener_with_shutdown(
                                app,
                                listener,
                                async move {
                                    let _ = shutdown_rx.await;
                                },
                            )
                            .await;
                            if let Err(err) = server_result {
                                tracing::error!("backend server stopped: {err}");
                            }
                            let action_workers = match state_for_thread.lock() {
                                Ok(mut state) => {
                                    state.shutting_down = true;
                                    std::mem::take(&mut state.action_workers)
                                }
                                Err(_) => Vec::new(),
                            };
                            join_action_workers(action_workers);
                            shutdown_until_clean(&cleanup_app).await;
                            if let Ok(mut state) = state_for_thread.lock() {
                                state.app = None;
                            }
                        }
                        Err(err) => tracing::error!("backend initialization failed: {err}"),
                    }
                });
            });
            if let Ok(mut state) = setup_state.lock() {
                state.worker = Some(worker);
            }

            Ok(())
        })
        .on_window_event(move |window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running rweb-clash desktop");
    shutdown_backend(&exit_state);
}

#[derive(Default)]
struct BackendState {
    app: Option<App>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    worker: Option<std::thread::JoinHandle<()>>,
    action_workers: Vec<std::thread::JoinHandle<()>>,
    shutting_down: bool,
    instance_lock: Option<InstanceLock>,
}

fn shutdown_backend(state: &Arc<Mutex<BackendState>>) {
    let (action_workers, shutdown_tx, worker, _lock) = match state.lock() {
        Ok(mut state) => {
            state.shutting_down = true;
            (
                std::mem::take(&mut state.action_workers),
                state.shutdown_tx.take(),
                state.worker.take(),
                state.instance_lock.take(),
            )
        }
        Err(_) => (Vec::new(), None, None, None),
    };
    join_action_workers(action_workers);
    if let Some(sender) = shutdown_tx {
        let _ = sender.send(());
    }
    if let Some(worker) = worker {
        if worker.join().is_err() {
            tracing::warn!("backend worker panicked during shutdown");
        }
    }
}

fn join_action_workers(workers: Vec<std::thread::JoinHandle<()>>) {
    for worker in workers {
        if worker.join().is_err() {
            tracing::warn!("tray backend action panicked during shutdown");
        }
    }
}

async fn shutdown_until_clean(app: &App) {
    loop {
        match app.shutdown().await {
            Ok(()) => break,
            Err(err) => {
                tracing::warn!("backend shutdown cleanup failed, retrying: {err}");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
    }
}

struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    fn acquire() -> anyhow::Result<Self> {
        let path = std::env::temp_dir().join("rweb-clash.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(anyhow::anyhow!(
                "another rweb-clash desktop instance is already running"
            )),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }
}

fn install_tray(app: &tauri::App, backend_state: Arc<Mutex<BackendState>>) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
    let start = MenuItem::with_id(app, "start_core", "启动内核", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop_core", "停止内核", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 R-Clash", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &start, &stop, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?;
    let state_for_menu = backend_state.clone();
    TrayIconBuilder::with_id("main")
        .tooltip("R-Clash")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "start_core" => run_backend_action(&state_for_menu, BackendAction::Start),
            "stop_core" => run_backend_action(&state_for_menu, BackendAction::Stop),
            "quit" => {
                shutdown_backend(&state_for_menu);
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

enum BackendAction {
    Start,
    Stop,
}

fn run_backend_action(state: &Arc<Mutex<BackendState>>, action: BackendAction) {
    let mut state = match state.lock() {
        Ok(state) => state,
        Err(_) => return,
    };
    if state.shutting_down {
        return;
    }
    let app = state.app.clone();
    let Some(app) = app else {
        return;
    };
    let worker = std::thread::spawn(move || {
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(err) => {
                tracing::warn!("failed to create tray action runtime: {err}");
                return;
            }
        };
        let result = runtime.block_on(async move {
            match action {
                BackendAction::Start => app.start_core().await.map(|_| ()),
                BackendAction::Stop => app.stop_core().await.map(|_| ()),
            }
        });
        if let Err(err) = result {
            tracing::warn!("tray backend action failed: {err}");
        }
    });
    state.action_workers.push(worker);
}

fn packaged_resource_dir(app: &tauri::App) -> Option<PathBuf> {
    let resource_dir = app.path().resource_dir().ok()?.join("resources");
    if resource_dir.is_dir() {
        return Some(resource_dir);
    }

    let dev_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
    if dev_dir.is_dir() {
        Some(dev_dir)
    } else {
        None
    }
}

fn listen_addr() -> SocketAddr {
    std::env::var("RWEB_CLASH_LISTEN")
        .ok()
        .and_then(|value| value.parse::<SocketAddr>().ok())
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 31990)))
}

fn init_logging() {
    let level = std::env::var("RWEB_CLASH_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .ok()
        .and_then(|value| parse_level(&value))
        .unwrap_or(Level::INFO);

    let _ = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_target(true)
        .compact()
        .try_init();
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
