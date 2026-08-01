#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
fn main() {
    eprintln!("rweb-clash-windows-helper can only run on Windows");
}

#[cfg(windows)]
mod windows_helper {
    use anyhow::{bail, Context, Result};
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use std::ffi::{OsStr, OsString};
    use std::fs::{File, OpenOptions};
    use std::io::{BufRead, BufReader, BufWriter, Write};
    use std::os::windows::io::{FromRawHandle, RawHandle};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::ptr::null_mut;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Duration;
    use windows_service::service::{
        ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
        ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_dispatcher;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, LocalFree, ERROR_PIPE_CONNECTED, HANDLE,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
        PIPE_TYPE_BYTE, PIPE_WAIT,
    };

    const SERVICE_NAME: &str = "rweb-clash-tun";
    const SERVICE_DISPLAY_NAME: &str = "rweb-clash TUN helper";
    const PIPE_NAME: &str = r"\\.\pipe\rweb-clash-tun";
    const MAX_MESSAGE_BYTES: u64 = 64 * 1024;
    static SERVICE_CONFIGURATION: OnceLock<ServiceConfig> = OnceLock::new();

    #[derive(Debug, Deserialize)]
    #[serde(tag = "op", rename_all = "snake_case")]
    enum Request {
        Ping,
        Stop,
        Status,
        Start {
            binary: String,
            config: String,
            state_dir: String,
            binary_sha256: String,
        },
    }

    #[derive(Debug, Serialize)]
    struct Response {
        ok: bool,
        error: Option<String>,
        pid: Option<u32>,
        running: Option<bool>,
    }

    #[derive(Debug, Clone)]
    struct ServiceConfig {
        root_dir: PathBuf,
        core_path: PathBuf,
        user_sid: String,
    }

    impl ServiceConfig {
        fn from_process_arguments(arguments: &[OsString]) -> Result<Self> {
            let root_dir = find_service_argument(arguments, "--root")?;
            let core_path = find_service_argument(arguments, "--core")?;
            let user_sid = find_service_argument(arguments, "--user-sid")?
                .to_string_lossy()
                .into_owned();
            if !user_sid.starts_with("S-") {
                bail!("invalid Windows user SID");
            }
            Ok(Self {
                root_dir: root_dir.into(),
                core_path: core_path.into(),
                user_sid,
            })
        }
    }

    pub fn main() -> Result<()> {
        let args = std::env::args_os().skip(1).collect::<Vec<_>>();
        match args.first().and_then(|arg| arg.to_str()) {
            Some("--install") => {
                let mut install_args = args.iter().skip(1).cloned();
                install_service(&mut install_args)
            }
            Some("--uninstall") => uninstall_service(),
            Some("--service") => {
                SERVICE_CONFIGURATION
                    .set(ServiceConfig::from_process_arguments(&args)?)
                    .map_err(|_| {
                        anyhow::anyhow!("Windows helper service configuration was set twice")
                    })?;
                service_dispatcher::start(SERVICE_NAME, ffi_service_main)
                    .context("start Windows service dispatcher")?;
                Ok(())
            }
            _ => {
                eprintln!(
                    "usage: rweb-clash-windows-helper --install --root PATH --core PATH --user-sid SID"
                );
                Ok(())
            }
        }
    }

    windows_service::define_windows_service!(ffi_service_main, service_main);

    fn service_main(arguments: Vec<OsString>) {
        if let Err(error) = service_main_inner(arguments) {
            eprintln!("rweb-clash TUN helper failed: {error:#}");
        }
    }

    fn service_main_inner(arguments: Vec<OsString>) -> Result<()> {
        let _ = arguments;
        let config = SERVICE_CONFIGURATION
            .get()
            .context("Windows helper service configuration is unavailable")?
            .clone();
        let stopping = Arc::new(AtomicBool::new(false));
        let process = Arc::new(Mutex::new(None::<Child>));
        let stopping_for_handler = stopping.clone();
        let process_for_handler = process.clone();
        let event_handler = move |event| match event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                stopping_for_handler.store(true, Ordering::Release);
                stop_process(&process_for_handler);
                wake_pipe();
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        };
        let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)
            .context("register service control handler")?;
        status_handle.set_service_status(running_status(ServiceState::StartPending))?;
        status_handle.set_service_status(running_status(ServiceState::Running))?;

        let result = run_server(&config, &stopping, &process);
        stop_process(&process);
        status_handle.set_service_status(running_status(ServiceState::Stopped))?;
        result
    }

    fn running_status(state: ServiceState) -> ServiceStatus {
        ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: if state == ServiceState::Running {
                ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
            } else {
                ServiceControlAccept::empty()
            },
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::from_secs(5),
            process_id: None,
        }
    }

    fn run_server(
        config: &ServiceConfig,
        stopping: &AtomicBool,
        process: &Arc<Mutex<Option<Child>>>,
    ) -> Result<()> {
        while !stopping.load(Ordering::Acquire) {
            let pipe = create_server_pipe(&config.user_sid)?;
            if stopping.load(Ordering::Acquire) {
                break;
            }
            serve_connection(pipe, config, process)?;
        }
        Ok(())
    }

    fn serve_connection(
        pipe: File,
        config: &ServiceConfig,
        process: &Arc<Mutex<Option<Child>>>,
    ) -> Result<()> {
        let reader = BufReader::new(pipe.try_clone()?);
        let mut writer = BufWriter::new(pipe);
        for line in reader.lines() {
            let line = line?;
            if line.len() > MAX_MESSAGE_BYTES as usize {
                write_response(
                    &mut writer,
                    Response {
                        ok: false,
                        error: Some("request is too large".into()),
                        pid: None,
                        running: None,
                    },
                )?;
                break;
            }
            let response = match serde_json::from_str::<Request>(&line) {
                Ok(Request::Ping) => Response {
                    ok: true,
                    error: None,
                    pid: None,
                    running: None,
                },
                Ok(Request::Status) => status_response(process),
                Ok(Request::Stop) => stop_response(process),
                Ok(Request::Start {
                    binary,
                    config: runtime_yaml,
                    state_dir,
                    binary_sha256,
                }) => start_response(
                    config,
                    process,
                    &binary,
                    &runtime_yaml,
                    &state_dir,
                    &binary_sha256,
                ),
                Err(error) => Response {
                    ok: false,
                    error: Some(error.to_string()),
                    pid: None,
                    running: None,
                },
            };
            write_response(&mut writer, response)?;
            break;
        }
        Ok(())
    }

    fn write_response(writer: &mut BufWriter<File>, response: Response) -> Result<()> {
        serde_json::to_writer(&mut *writer, &response)?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        Ok(())
    }

    fn status_response(process: &Arc<Mutex<Option<Child>>>) -> Response {
        let mut guard = process.lock().expect("helper process mutex poisoned");
        let running = match guard.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(_)) => {
                    *guard = None;
                    false
                }
                Ok(None) => true,
                Err(_) => false,
            },
            None => false,
        };
        Response {
            ok: true,
            error: None,
            pid: guard.as_ref().map(Child::id),
            running: Some(running),
        }
    }

    fn stop_response(process: &Arc<Mutex<Option<Child>>>) -> Response {
        stop_process(process);
        Response {
            ok: true,
            error: None,
            pid: None,
            running: Some(false),
        }
    }

    fn stop_process(process: &Arc<Mutex<Option<Child>>>) {
        let child = process
            .lock()
            .expect("helper process mutex poisoned")
            .take();
        if let Some(mut child) = child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn start_response(
        service_config: &ServiceConfig,
        process: &Arc<Mutex<Option<Child>>>,
        binary: &str,
        runtime_yaml: &str,
        state_dir: &str,
        expected_hash: &str,
    ) -> Response {
        if let Err(error) = validate_request(
            service_config,
            binary,
            runtime_yaml,
            state_dir,
            expected_hash,
        ) {
            return Response {
                ok: false,
                error: Some(error.to_string()),
                pid: None,
                running: None,
            };
        }
        let mut guard = process.lock().expect("helper process mutex poisoned");
        if let Some(child) = guard.as_mut() {
            if child.try_wait().ok().flatten().is_none() {
                return Response {
                    ok: false,
                    error: Some("TUN helper already has a running Mihomo process".into()),
                    pid: Some(child.id()),
                    running: Some(true),
                };
            }
            *guard = None;
        }

        let log_path = Path::new(state_dir).join("mihomo.log");
        let log = match OpenOptions::new().create(true).append(true).open(&log_path) {
            Ok(log) => log,
            Err(error) => return failed_response(format!("failed to open Mihomo log: {error}")),
        };
        let error_log = match log.try_clone() {
            Ok(log) => log,
            Err(error) => return failed_response(format!("failed to prepare Mihomo log: {error}")),
        };
        match Command::new(binary)
            .args(["-d", state_dir, "-f", runtime_yaml])
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(error_log))
            .spawn()
        {
            Ok(child) => {
                let pid = child.id();
                *guard = Some(child);
                Response {
                    ok: true,
                    error: None,
                    pid: Some(pid),
                    running: Some(true),
                }
            }
            Err(error) => failed_response(format!("failed to start Mihomo: {error}")),
        }
    }

    fn failed_response(error: String) -> Response {
        Response {
            ok: false,
            error: Some(error),
            pid: None,
            running: None,
        }
    }

    fn validate_request(
        service_config: &ServiceConfig,
        binary: &str,
        runtime_yaml: &str,
        state_dir: &str,
        expected_hash: &str,
    ) -> Result<()> {
        let binary = canonical_file(binary).context("invalid Mihomo binary path")?;
        let runtime_yaml = canonical_file(runtime_yaml).context("invalid runtime config path")?;
        let state_dir = canonical_directory(state_dir).context("invalid runtime directory")?;
        let root_dir = canonical_directory(&service_config.root_dir)
            .context("managed application data directory is unavailable")?;
        let core_path = canonical_file(&service_config.core_path)
            .context("packaged Mihomo core is unavailable")?;
        if !same_path(&binary, &core_path) {
            bail!("Mihomo binary is outside the protected installation resource");
        }
        if !same_path(&state_dir, &root_dir.join("data").join("profiles"))
            || !same_path(&runtime_yaml, &state_dir.join("runtime.yaml"))
        {
            bail!("TUN runtime paths are outside the managed application data directory");
        }
        let bytes = std::fs::read(&binary).context("read Mihomo binary")?;
        let actual = hex::encode(Sha256::digest(bytes));
        if actual != expected_hash {
            bail!("Mihomo binary integrity check failed");
        }
        Ok(())
    }

    fn canonical_file(path: impl AsRef<Path>) -> Result<PathBuf> {
        let path = path.as_ref();
        if !path.is_absolute() || !path.is_file() {
            bail!("path must be an existing absolute file: {}", path.display());
        }
        Ok(std::fs::canonicalize(path)?)
    }

    fn canonical_directory(path: impl AsRef<Path>) -> Result<PathBuf> {
        let path = path.as_ref();
        if !path.is_absolute() || !path.is_dir() {
            bail!(
                "path must be an existing absolute directory: {}",
                path.display()
            );
        }
        Ok(std::fs::canonicalize(path)?)
    }

    fn same_path(left: &Path, right: &Path) -> bool {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }

    fn create_server_pipe(user_sid: &str) -> Result<File> {
        let sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{user_sid})");
        let sddl_wide = wide_null(&sddl);
        let mut descriptor = null_mut();
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_wide.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        };
        if converted == 0 {
            bail!(
                "create TUN helper pipe security descriptor failed: {}",
                last_error()
            );
        }
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let pipe_name = wide_null(PIPE_NAME);
        let pipe = unsafe {
            CreateNamedPipeW(
                pipe_name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                64 * 1024,
                64 * 1024,
                0,
                &mut attributes,
            )
        };
        unsafe {
            LocalFree(descriptor);
        }
        if pipe == -1isize as HANDLE {
            bail!("create TUN helper pipe failed: {}", last_error());
        }
        let connected = unsafe { ConnectNamedPipe(pipe, null_mut()) };
        if connected == 0 && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
            unsafe {
                CloseHandle(pipe);
            }
            bail!("connect TUN helper pipe failed: {}", last_error());
        }
        let file = unsafe { File::from_raw_handle(pipe as RawHandle) };
        Ok(file)
    }

    fn wake_pipe() {
        let _ = OpenOptions::new()
            .read(true)
            .write(true)
            .open(PIPE_NAME)
            .and_then(|mut pipe| pipe.write_all(b"{\"op\":\"Ping\"}\n"));
    }

    fn install_service(args: &mut impl Iterator<Item = OsString>) -> Result<()> {
        let root_dir = required_argument(args, "--root")?;
        let core_path = required_argument(args, "--core")?;
        let root_dir = canonical_directory(root_dir).context("validate managed data root")?;
        let core_path = canonical_file(core_path).context("validate packaged Mihomo core")?;
        let user_sid = required_argument_string(args, "--user-sid")?;
        if !user_sid.starts_with("S-") {
            bail!("invalid Windows user SID");
        }
        let executable_path = std::env::current_exe().context("locate helper executable")?;
        let manager = ServiceManager::local_computer(
            None::<&str>,
            ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
        )?;
        remove_existing_service(&manager)?;
        let service_info = ServiceInfo {
            name: OsString::from(SERVICE_NAME),
            display_name: OsString::from(SERVICE_DISPLAY_NAME),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path,
            launch_arguments: vec![
                OsString::from("--service"),
                OsString::from("--root"),
                root_dir.into_os_string(),
                OsString::from("--core"),
                core_path.into_os_string(),
                OsString::from("--user-sid"),
                OsString::from(user_sid),
            ],
            dependencies: vec![],
            account_name: None,
            account_password: None,
        };
        let service = manager.create_service(
            &service_info,
            ServiceAccess::START | ServiceAccess::QUERY_STATUS | ServiceAccess::STOP,
        )?;
        service.start::<&OsStr>(&[])?;
        Ok(())
    }

    fn remove_existing_service(manager: &ServiceManager) -> Result<()> {
        let access = ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS;
        let Ok(service) = manager.open_service(SERVICE_NAME, access) else {
            return Ok(());
        };
        let _ = service.stop();
        for _ in 0..50 {
            if service.query_status()?.current_state == ServiceState::Stopped {
                return Ok(service.delete()?);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        bail!("Windows TUN helper service did not stop in time")
    }

    fn uninstall_service() -> Result<()> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
        remove_existing_service(&manager)
    }

    fn required_argument(args: &mut impl Iterator<Item = OsString>, name: &str) -> Result<PathBuf> {
        let flag = args
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing {name}"))?;
        if flag != OsString::from(name) {
            bail!("expected {name}, got {}", flag.to_string_lossy());
        }
        Ok(PathBuf::from(args.next().ok_or_else(|| {
            anyhow::anyhow!("missing value for {name}")
        })?))
    }

    fn required_argument_string(
        args: &mut impl Iterator<Item = OsString>,
        name: &str,
    ) -> Result<String> {
        Ok(required_argument(args, name)?
            .to_string_lossy()
            .into_owned())
    }

    fn find_service_argument(arguments: &[OsString], name: &str) -> Result<OsString> {
        let position = arguments
            .iter()
            .position(|argument| argument == &OsString::from(name))
            .ok_or_else(|| anyhow::anyhow!("missing {name}"))?;
        arguments
            .get(position + 1)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing value for {name}"))
    }

    fn wide_null(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn last_error() -> std::io::Error {
        std::io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
    }
}

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_helper::main() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
