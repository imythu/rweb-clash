# Tauri 打包

Tauri 负责 macOS arm64 和 Windows amd64。Linux 不使用 Tauri，完整流程见 [`../README.md`](../README.md)。

## 打包职责

1. 构建 `web/dist`。
2. 将平台对应的 Mihomo core 放入 Tauri resources。
3. 将默认规则集缓存和 manifest 放入 Tauri resources。
4. 启动时把 resources 复制到平台 app data dir。
5. 启动共享后端服务，并让前端继续调用同一套 API。

## 平台边界

macOS 和 Windows 使用 Mihomo sidecar 模式。移动端暂不纳入当前打包范围。
