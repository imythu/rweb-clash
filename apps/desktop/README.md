# desktop

Windows amd64 和 macOS arm64 的 Tauri 应用打包入口。

现有前端仍然放在 `web/`。Tauri 启动时会初始化共享 Rust 后端并托管本地 API，前端继续通过 `/api` 访问同一套能力。
