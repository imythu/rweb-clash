# 后端骨架

后端围绕共享服务拆分，让同一套核心逻辑可以支撑两种打包模式。

## 打包模式

- Linux：先构建前端，再产出一个 Rust 可执行文件，同时提供 API 路由和已构建的静态资源。
- Windows/macOS/Android：使用 Tauri 打包现有前端，并从应用运行时调用 Rust 后端层。

## 计划模块

- `crates/rweb-clash`：共享库，放核心业务、Clash/Mihomo 集成、配置、订阅、规则、日志、流量和连接等能力。
- `crates/rweb-clash-bin`：Linux/HTTP 二进制入口，负责启动 API 服务，并在 Linux 单可执行文件模式下托管已构建的前端资源。
- `apps/desktop`：Tauri 应用入口，用于 Windows、macOS 和 Android 打包，复用共享库里的后端能力。

## 当前范围

当前只加入项目骨架和打包目录布局，暂不加入 API 实现和 Rust 源码文件。
