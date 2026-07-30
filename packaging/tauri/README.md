# Tauri 打包

Tauri 负责 macOS arm64 和 Windows amd64。Linux 不使用 Tauri，完整流程见 [`../README.md`](../README.md)。

## 打包职责

1. 构建 `web/dist`。
2. 将平台对应的 Mihomo core 放入 Tauri resources。
3. 将默认规则集缓存和 manifest 放入 Tauri resources。
4. 将构建时校验过的 `runtime/geoip.metadb` 和 manifest 放入 Tauri resources。
5. 启动时把 resources 复制到平台 app data dir。
6. 启动共享后端服务，并让前端继续调用同一套 API。

## 平台边界

macOS 和 Windows 使用 Mihomo sidecar 模式。移动端暂不纳入当前打包范围。

macOS 启用 TUN 时会通过系统授权对话框单独提升 Mihomo。授权后的二进制、运行配置和 GeoIP 会在 root 专属临时目录中校验后运行，不会提升 Tauri 主进程；停止内核、退出应用或主进程异常终止时都会回收该特权 Mihomo 进程。

正式签名构建会在 Tauri 打包前单独签名 Mihomo 可执行资源，避免已签名应用首次启动内核时再次触发 Gatekeeper。未签名构建仍需按 macOS 提示手动允许应用和内核。
