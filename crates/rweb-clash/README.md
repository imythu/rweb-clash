# rweb-clash

共享库 crate。

计划职责：

- 放置 Clash/Mihomo 集成、配置管理、订阅管理、规则管理、日志、流量、连接等核心能力。
- 提供 HTTP 二进制和 Tauri 应用共同复用的服务接口。
- 保持业务逻辑与具体运行入口解耦，避免 Linux 单可执行文件和 Tauri 应用各写一套后端逻辑。

内核通过随包携带的 Mihomo core 运行，并由后端管理进程与 controller 交互。
