# R-Clash

R-Clash 是一个面向普通用户和进阶用户的 Clash/Mihomo 客户端。

## 核心设计理念

**系统主导（控制面），Clash 只是工具（数据面）**

*   **单一事实来源 (Source of Truth)**：所有的节点、分组、规则和配置意图完全由 R-Clash 系统（本地 SQLite 数据库）主导和管理。
*   **运行时配置为临时编译产物**：Clash 所需的 `runtime.yaml` 仅仅是系统根据数据库状态按需生成的“临时编译产物”。绝不允许反向从 `runtime.yaml` 读取状态或允许直接编辑底层配置文件。
*   **计算在系统，执行在内核**：复杂的节点清洗、分组拓扑计算、规则集合并等逻辑，全部在 R-Clash 系统内完成。系统喂给 Clash 的是最纯粹、已计算好的静态路由表。
*   **基于 API 的微操指挥**：深度利用 Mihomo `external-controller` (REST API)。通过 API 进行配置平滑热更新、劫持代理组节点切换、高频提取实时流量和连接状态，将内核视为纯粹受系统调度的“黑盒路由器”。

## 文档

请参阅 `docs/` 目录获取更多设计细节：
*   [功能设计](docs/功能设计.md)
*   [数据库设计](docs/数据库设计.md)
*   [Clash 交互设计](docs/Clash交互设计.md)
*   [后端骨架](docs/backend-skeleton.md)
*   [打包流程](packaging/README.md)
*   [发布检查清单](packaging/release-checklist.md)

## 本地开发

后端默认监听 `127.0.0.1:31990`：

```text
cargo run -p rweb-clash-bin
```

前端通过 Vite 代理访问本地后端：

```text
cd web
corepack pnpm install --frozen-lockfile
corepack pnpm dev
```

提交前质量门禁：

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
corepack pnpm --dir web lint
corepack pnpm --dir web build
```

## 安全默认

- 回环地址上的桌面/本地服务可直接访问；监听 `0.0.0.0` 或其他非回环地址时，必须设置至少 16 字符的 `RWEB_CLASH_API_TOKEN`。
- 跨域 API 仅允许 Tauri 和本地 Vite 来源。额外来源通过 `RWEB_CLASH_ALLOWED_ORIGINS` 以逗号分隔配置。
- 订阅与规则集下载会校验每次重定向和 DNS 结果，并限制并发、总时长和响应大小。私网、回环及保留地址默认拒绝；仅对可信本地源设置 `RWEB_CLASH_ALLOW_PRIVATE_SOURCES=1`。
- 启用系统代理前会在数据目录持久化原设置，停止、退出或 Mihomo 异常退出时恢复；备份仅在恢复成功后删除，Unix 权限为 `0600`。
- Docker 默认只映射到宿主 `127.0.0.1`，并要求 API 令牌。完整示例见 [`packaging/linux/README.md`](packaging/linux/README.md)。
