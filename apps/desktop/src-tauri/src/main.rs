#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rweb_clash::{types::SystemConfigPatch, App, AppOptions};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use tauri_plugin_autostart::ManagerExt;
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
    let started_from_autostart = std::env::args_os().any(|arg| arg == "--autostart");
    tauri::Builder::default()
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .arg("--autostart")
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_desktop_preferences,
            set_launch_at_login,
            set_silent_start
        ])
        .setup(move |app| {
            let app_data_dir = app.path().app_data_dir()?;
            let preferences_path = app_data_dir.join("desktop-preferences.json");
            let mut preferences =
                load_desktop_preferences(&preferences_path).unwrap_or_else(|err| {
                    tracing::warn!("failed to load desktop preferences: {err}");
                    DesktopPreferences::default()
                });
            match app.autolaunch().is_enabled() {
                Ok(enabled) if enabled != preferences.launch_at_login => {
                    preferences.launch_at_login = enabled;
                    if let Err(err) = save_desktop_preferences(&preferences_path, &preferences) {
                        tracing::warn!("failed to reconcile desktop preferences: {err}");
                    }
                }
                Ok(_) => {}
                Err(err) => tracing::warn!("failed to read autostart state: {err}"),
            }
            let should_start_silently = started_from_autostart && preferences.silent_start;
            app.manage(DesktopPreferencesState {
                path: preferences_path,
                value: Mutex::new(preferences),
            });
            if let Ok(mut state) = setup_state.lock() {
                state.instance_lock = Some(instance_lock);
            }
            let tray_items = install_tray(app, tray_state.clone())?;
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
                state.tray_worker = Some(start_tray_monitor(
                    app.handle().clone(),
                    tray_state.clone(),
                    state.tray_stop.clone(),
                    tray_items,
                ));
            }

            if !should_start_silently {
                show_main_window(app.handle());
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct DesktopPreferences {
    launch_at_login: bool,
    silent_start: bool,
}

struct DesktopPreferencesState {
    path: PathBuf,
    value: Mutex<DesktopPreferences>,
}

fn load_desktop_preferences(path: &Path) -> anyhow::Result<DesktopPreferences> {
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(DesktopPreferences::default()),
        Err(err) => Err(err.into()),
    }
}

fn save_desktop_preferences(path: &Path, preferences: &DesktopPreferences) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(preferences)?)?;
    Ok(())
}

#[tauri::command]
fn get_desktop_preferences(
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopPreferencesState>,
) -> Result<DesktopPreferences, String> {
    let launch_at_login = app
        .autolaunch()
        .is_enabled()
        .map_err(|err| err.to_string())?;
    let mut preferences = state
        .value
        .lock()
        .map_err(|_| "desktop preferences lock is poisoned".to_string())?;
    if preferences.launch_at_login != launch_at_login {
        let mut next = preferences.clone();
        next.launch_at_login = launch_at_login;
        save_desktop_preferences(&state.path, &next).map_err(|err| err.to_string())?;
        *preferences = next;
    }
    Ok(preferences.clone())
}

#[tauri::command]
fn set_launch_at_login(
    enabled: bool,
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopPreferencesState>,
) -> Result<DesktopPreferences, String> {
    let manager = app.autolaunch();
    let was_enabled = manager.is_enabled().map_err(|err| err.to_string())?;
    if enabled != was_enabled {
        if enabled {
            manager.enable()
        } else {
            manager.disable()
        }
        .map_err(|err| err.to_string())?;
    }

    let mut preferences = state
        .value
        .lock()
        .map_err(|_| "desktop preferences lock is poisoned".to_string())?;
    let mut next = preferences.clone();
    next.launch_at_login = enabled;
    if let Err(err) = save_desktop_preferences(&state.path, &next) {
        let rollback = if was_enabled {
            manager.enable()
        } else {
            manager.disable()
        };
        if let Err(rollback_err) = rollback {
            tracing::warn!(
                "failed to roll back autostart after preference save error: {rollback_err}"
            );
        }
        return Err(err.to_string());
    }
    *preferences = next.clone();
    Ok(next)
}

#[tauri::command]
fn set_silent_start(
    enabled: bool,
    state: tauri::State<'_, DesktopPreferencesState>,
) -> Result<DesktopPreferences, String> {
    let mut preferences = state
        .value
        .lock()
        .map_err(|_| "desktop preferences lock is poisoned".to_string())?;
    let mut next = preferences.clone();
    next.silent_start = enabled;
    save_desktop_preferences(&state.path, &next).map_err(|err| err.to_string())?;
    *preferences = next.clone();
    Ok(next)
}

#[derive(Default)]
struct BackendState {
    app: Option<App>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    worker: Option<std::thread::JoinHandle<()>>,
    tray_worker: Option<std::thread::JoinHandle<()>>,
    tray_stop: Arc<AtomicBool>,
    action_workers: Vec<std::thread::JoinHandle<()>>,
    shutting_down: bool,
    instance_lock: Option<InstanceLock>,
}

fn shutdown_backend(state: &Arc<Mutex<BackendState>>) {
    let (action_workers, tray_worker, shutdown_tx, worker, _lock) = match state.lock() {
        Ok(mut state) => {
            state.shutting_down = true;
            state.tray_stop.store(true, Ordering::Release);
            (
                std::mem::take(&mut state.action_workers),
                state.tray_worker.take(),
                state.shutdown_tx.take(),
                state.worker.take(),
                state.instance_lock.take(),
            )
        }
        Err(_) => (Vec::new(), None, None, None, None),
    };
    join_action_workers(action_workers);
    if let Some(worker) = tray_worker {
        if worker.join().is_err() {
            tracing::warn!("tray monitor panicked during shutdown");
        }
    }
    if let Some(sender) = shutdown_tx {
        let _ = sender.send(());
    }
    if let Some(worker) = worker {
        if worker.join().is_err() {
            tracing::warn!("backend worker panicked during shutdown");
        }
    }
}

fn exit_after_tray_stops(app: &tauri::AppHandle, state: &Arc<Mutex<BackendState>>) {
    let tray_worker = match state.lock() {
        Ok(mut state) => {
            if state.shutting_down {
                return;
            }
            state.shutting_down = true;
            state.tray_stop.store(true, Ordering::Release);
            state.tray_worker.take()
        }
        Err(_) => None,
    };
    let app = app.clone();
    std::thread::spawn(move || {
        if let Some(worker) = tray_worker {
            if worker.join().is_err() {
                tracing::warn!("tray monitor panicked during exit");
            }
        }
        app.exit(0);
    });
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

#[derive(Clone)]
struct TrayMenuItems {
    speed: MenuItem<tauri::Wry>,
    system_proxy: CheckMenuItem<tauri::Wry>,
    tun: CheckMenuItem<tauri::Wry>,
    mode_rule: CheckMenuItem<tauri::Wry>,
    mode_global: CheckMenuItem<tauri::Wry>,
    mode_direct: CheckMenuItem<tauri::Wry>,
}

fn install_tray(
    app: &tauri::App,
    backend_state: Arc<Mutex<BackendState>>,
) -> tauri::Result<TrayMenuItems> {
    let speed = MenuItem::with_id(
        app,
        "traffic",
        "实时速度：↓ 0 B/s  ↑ 0 B/s",
        false,
        None::<&str>,
    )?;
    let system_proxy =
        CheckMenuItem::with_id(app, "system_proxy", "系统代理", true, false, None::<&str>)?;
    let tun = CheckMenuItem::with_id(app, "tun", "TUN 模式", true, false, None::<&str>)?;
    let mode_rule =
        CheckMenuItem::with_id(app, "mode_rule", "规则模式", true, false, None::<&str>)?;
    let mode_global =
        CheckMenuItem::with_id(app, "mode_global", "全局模式", true, false, None::<&str>)?;
    let mode_direct =
        CheckMenuItem::with_id(app, "mode_direct", "直连模式", true, false, None::<&str>)?;
    let mode = Submenu::with_items(
        app,
        "运行模式",
        true,
        &[&mode_rule, &mode_global, &mode_direct],
    )?;
    let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
    let start = MenuItem::with_id(app, "start_core", "启动内核", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop_core", "停止内核", true, None::<&str>)?;
    let restart = MenuItem::with_id(app, "restart_core", "重启内核", true, None::<&str>)?;
    let close_connections =
        MenuItem::with_id(app, "close_connections", "关闭全部连接", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 R-Clash", true, None::<&str>)?;
    let separator_a = PredefinedMenuItem::separator(app)?;
    let separator_b = PredefinedMenuItem::separator(app)?;
    let separator_c = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &speed,
            &separator_a,
            &system_proxy,
            &tun,
            &mode,
            &separator_b,
            &start,
            &stop,
            &restart,
            &close_connections,
            &separator_c,
            &show,
            &quit,
        ],
    )?;
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
            "restart_core" => run_backend_action(&state_for_menu, BackendAction::Restart),
            "system_proxy" => run_backend_action(&state_for_menu, BackendAction::ToggleSystemProxy),
            "tun" => run_backend_action(&state_for_menu, BackendAction::ToggleTun),
            "mode_rule" => run_backend_action(&state_for_menu, BackendAction::SetMode("rule")),
            "mode_global" => run_backend_action(&state_for_menu, BackendAction::SetMode("global")),
            "mode_direct" => run_backend_action(&state_for_menu, BackendAction::SetMode("direct")),
            "close_connections" => {
                run_backend_action(&state_for_menu, BackendAction::CloseConnections)
            }
            "quit" => exit_after_tray_stops(app, &state_for_menu),
            _ => {}
        })
        .build(app)?;
    Ok(TrayMenuItems {
        speed,
        system_proxy,
        tun,
        mode_rule,
        mode_global,
        mode_direct,
    })
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
    Restart,
    ToggleSystemProxy,
    ToggleTun,
    SetMode(&'static str),
    CloseConnections,
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
                BackendAction::Restart => app.restart_core().await.map(|_| ()),
                BackendAction::ToggleSystemProxy => {
                    let config = app.config().await?;
                    app.update_config(SystemConfigPatch {
                        system_proxy: Some(!config.system_proxy),
                        ..SystemConfigPatch::default()
                    })
                    .await
                    .map(|_| ())
                }
                BackendAction::ToggleTun => {
                    let config = app.config().await?;
                    app.update_config(SystemConfigPatch {
                        tun: Some(!config.tun),
                        ..SystemConfigPatch::default()
                    })
                    .await
                    .map(|_| ())
                }
                BackendAction::SetMode(mode) => app
                    .update_config(SystemConfigPatch {
                        mode: Some(mode.to_string()),
                        ..SystemConfigPatch::default()
                    })
                    .await
                    .map(|_| ()),
                BackendAction::CloseConnections => app.close_all_connections().await,
            }
        });
        if let Err(err) = result {
            tracing::warn!("tray backend action failed: {err}");
        }
    });
    state.action_workers.push(worker);
}

fn start_tray_monitor(
    app_handle: tauri::AppHandle,
    backend_state: Arc<Mutex<BackendState>>,
    stop: Arc<AtomicBool>,
    items: TrayMenuItems,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Runtime::new() {
            Ok(runtime) => runtime,
            Err(err) => {
                tracing::warn!("failed to create tray monitor runtime: {err}");
                return;
            }
        };

        while !stop.load(Ordering::Acquire) {
            let backend = backend_state
                .lock()
                .ok()
                .and_then(|state| state.app.clone());
            let (config, up, down) = if let Some(backend) = backend {
                runtime.block_on(async move {
                    let config = backend.config().await.ok();
                    let traffic = backend.traffic().await;
                    (config, traffic.up, traffic.down)
                })
            } else {
                (None, 0, 0)
            };

            update_tray(&app_handle, &items, config, up, down);
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    })
}

fn update_tray(
    app_handle: &tauri::AppHandle,
    items: &TrayMenuItems,
    config: Option<rweb_clash::types::SystemConfig>,
    up: u64,
    down: u64,
) {
    let speed = format!("↓ {}  ↑ {}", format_rate(down), format_rate(up));
    let menu_text = format!("实时速度：{speed}");
    let tooltip = format!("R-Clash\n{speed}");
    let _ = items.speed.set_text(menu_text);
    let (system_proxy, tun, mode) = config
        .map(|config| (config.system_proxy, config.tun, config.mode))
        .unwrap_or_else(|| (false, false, String::new()));
    let _ = items.system_proxy.set_checked(system_proxy);
    let _ = items.tun.set_checked(tun);
    let _ = items.mode_rule.set_checked(mode == "rule");
    let _ = items.mode_global.set_checked(mode == "global");
    let _ = items.mode_direct.set_checked(mode == "direct");
    if let Some(tray) = app_handle.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tooltip));
    }
}

fn format_rate(bytes_per_second: u64) -> String {
    const UNITS: [&str; 4] = ["B/s", "KB/s", "MB/s", "GB/s"];
    let mut value = bytes_per_second as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
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
