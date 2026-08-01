# desktop

Windows amd64 和 macOS arm64 的 Tauri 应用打包入口。

现有前端仍然放在 `web/`。Tauri 启动时会初始化共享 Rust 后端并托管本地 API，前端继续通过 `/api` 访问同一套能力。

Windows TUN 使用 `resources/windows/rweb-clash-windows-helper.exe`。首次开启 TUN 时仅通过一次 UAC 安装 `rweb-clash-tun` 服务；日常桌面进程保持普通权限，通过仅允许当前桌面用户 SID 的 named pipe 控制服务。
