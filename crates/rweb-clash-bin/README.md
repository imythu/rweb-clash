# rweb-clash-bin

Linux/HTTP 二进制 crate。

当前职责：

- 解析命令行参数和 `RWEB_CLASH_*` 环境变量。
- 启动 HTTP API 服务，供现有前端调用。
- 在 Linux 打包模式下托管 `web/dist` 静态资源。
- 通过 `embedded-assets` feature 将前端、Mihomo core 和默认规则集打包到单个可执行文件。
