# src-tauri

Tauri 桌面运行时实现。

当前职责：

- 初始化并关闭共享的 `rweb-clash` Rust 后端。
- 在 `127.0.0.1:31990` 提供本地 API。
- 管理单实例锁、托盘菜单和窗口生命周期。
- 将 Mihomo core 与默认规则集作为平台资源打包。

当前发布目标：

- Windows amd64 应用包。
- macOS arm64 应用包。
