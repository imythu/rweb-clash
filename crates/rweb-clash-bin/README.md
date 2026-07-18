# rweb-clash-bin

Linux/HTTP 二进制 crate。

当前职责：

- 解析命令行参数和 `RWEB_CLASH_*` 环境变量。
- 启动 HTTP API 服务，供现有前端调用。
- 在 Linux 打包模式下托管 `web/dist` 静态资源。
- 通过 `embedded-assets` feature 将前端、Mihomo core、GeoIP 数据库和 13 个默认规则集打包到单个可执行文件。
- Mihomo 配置校验默认等待 120 秒，可通过 `RWEB_CLASH_MIHOMO_VALIDATION_TIMEOUT_SECS` 在 1 到 3600 秒之间调整。
- `--wait-api <1..3600>` 只等待可管理的 HTTP API，供主 systemd service 保留故障修复入口；`--wait-ready <1..3600>` 还会在 core auto-start 已启用时要求 Mihomo 进入 `running`，供 Docker/Containerd readiness gate 使用。两者都不会启动第二个服务实例。
- Release 的 Linux amd64/arm64 目标使用 musl 静态链接，并同时提供独立二进制与 systemd 安装包。
