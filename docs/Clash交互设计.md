# Clash/Mihomo 交互设计

本文档描述后端与 Clash/Mihomo 的交互方式。

**核心设计理念：系统主导（控制面），Clash 只是工具（数据面）。**
R-Clash 是一个完整的本地客户端，绝不依赖外部系统来管理配置。系统的本地 SQLite 数据库是唯一的“事实来源”。Mihomo 需要的 `runtime.yaml` 仅仅是被系统按需生成的“临时编译产物”。

## 1. 交互对象

后端需要和两类对象交互：

- Mihomo 进程：负责代理、TUN、DNS、路由和连接处理。
- Mihomo external-controller：负责运行态查询和动态操作（热更新、节点切换、状态提取等）。

不提供“编辑原始 Clash 配置文件”的功能。所有交互必须通过以下方式收敛：

1. 用户操作和订阅数据写入 SQLite 数据库。
2. 后端在需要时（启动或配置变更时），将系统状态“编译”生成最纯粹的运行时 YAML。
3. 后端启动或通过 Controller API 热更新 Mihomo。
4. UI 的运行态数据、节点切换指令均通过 Controller API 微操完成，不直接写文件。

## 2. 文件布局

建议本地目录分为：

```text
data/
  app.db
  profiles/
    runtime.yaml
    rule-sets/
      {rule_set_id}.list
  logs/
    app.log
cache-core/
  mihomo
```

说明：

- `app.db` 保存系统唯一事实来源。
- `runtime.yaml` 是后端动态生成的临时编译产物，不应被人工修改，随时可被覆盖。
- `profiles/rule-sets/` 保存规则集文件，数据库只保存路径，配置中通过路径引用。
- `cache-core/` 保存随包附带或下载的 Mihomo 二进制。

## 3. 启动流程

启动后端时：

1. 初始化数据目录。
2. 初始化数据库和默认配置。
3. 检查 Mihomo 二进制是否存在。
4. 读取 `app_settings`。
5. 如果配置要求自动启动内核，则生成 `runtime.yaml`。
6. 启动 Mihomo 进程。
7. 轮询 controller `/version`，确认内核就绪。
8. 启动日志、流量采样、规则集刷新等后台任务。

## 4. 配置生成 (编译临时产物)

生成 `runtime.yaml` 时，后端按以下顺序“编译”：

1. 读取系统配置：端口、DNS、TUN、日志等级、模式等。
2. 查询经过清洗和合并后的统一资产池 `proxy_items`（其中 `kind='node'` 和 `kind='group'`）。
3. 解析用户自定义分组的过滤逻辑，计算出最终的确定性节点名称列表。
4. 生成 `proxies` 和 `proxy-groups`：写入给 Mihomo 的只是静态打平的列表，不包含任何复杂的系统侧业务逻辑。
5. 生成 `rule-providers`，引用本地 `profiles/rule-sets/` 目录的规则集文件。
6. 提取 `routing_rules` 生成用户路由规则。
7. 写入临时文件并原子替换 `runtime.yaml`。

原则：

- 代理组成员按名称输出，系统负责确保 `proxy_items` 里的全局名称唯一。
- 系统完成所有复杂的逻辑计算，喂给 Clash 的是最简单的映射关系。
- 生成 YAML 前必须校验所有 `proxy_group_members.member_name` 都能解析为 `proxy_items.name` 或 Mihomo 保留名称，例如 `DIRECT`。如果发现未知的孤儿成员名称，生成 YAML 时忽略并写日志；清理由订阅刷新或分组重算流程处理。
- 生成 YAML 前必须检测代理组引用图，拒绝循环引用。
- 生成 YAML 前必须校验 `routing_rules.policy` 指向有效出站名称或 Mihomo 内置策略。如果引用的出站对象已不存在（如被删除），系统应将该规则的策略默认降级为 `DIRECT` 或 `REJECT`，并在日志中输出警告，以避免内核启动失败。

## 5. 基于 API 的微操热更新策略

优先使用 controller 热更新，避免重启断开活跃连接：

```http
PUT /configs
Content-Type: application/json

// 示意，具体 payload 需以 Mihomo 实际 API 为准
{
  "path": "data/profiles/runtime.yaml"
}
```

系统在发生以下变化时，生成新的 YAML 后会调用热更新：
- 订阅刷新、节点清洗后拓扑变化。
- 自定义分组变化。
- 路由规则变化。
- 规则集刷新。
- 从日志中快捷添加了新的直连/拦截规则。

如果热更新失败或进行了不支持热更新的更改（如改了混淆端口、TUN 开关）：
1. 后端退回“停止并重启 Mihomo”。
2. 记录日志。

## 6. Controller API 映射

后端对前端暴露自己的 API，不直接暴露 Mihomo controller。后端负责适配、劫持指令和加工数据。

| 前端 API | Mihomo controller | 用途 |
| --- | --- | --- |
| `GET /api/core/status` | `/version`、进程状态 | 查询内核状态 |
| `GET /api/traffic` | `/traffic` | 轮询查询实时流量（不落库） |
| `GET /api/connections` | `/connections` | 轮询查询活跃连接（不落库） |
| `DELETE /api/connections/{id}` | `/connections/{id}` | 关闭连接 |
| `PUT /api/proxies/{group}` | `/proxies/{group}` | 劫持代理组切换，**直接通过 API 通知内核换节点** |
| `POST /api/nodes/test` | `/proxies/{name}/delay` | 单节点测速 |
| `POST /api/proxies/{group}/test` | `/group/{name}/delay` 或逐节点测速 | 分组测速 |

## 7. 代理与分组计算

系统中的 `proxy_items` (kind: 'node', 'group') 和 `proxy_group_filters` / `proxy_group_members` 决定最终的拓扑。

生成和切换逻辑：

- 订阅下载后，在**系统层**运用规则（正则剔除、清洗等），干净的节点存入 `proxy_items`。
- **系统层**计算用户自定义的分组，将结果打平后写入 `runtime.yaml`。
- 运行态中，前端要求切换代理出口节点时，后端接收指令，直接调用 Controller API 通知 Mihomo，而不是重新生成配置。

## 8. 规则与规则集托管

- `routing_rules` 表维护顺序，按序生成。
- 规则集订阅由**系统后端**负责下载和更新。文件存到本地，`rule_sets` 表保存路径。
- 生成配置时写入 `rule-providers` 指向本地文件，极大地减少了 `runtime.yaml` 的体积和 Mihomo 的解析压力。

## 9. 日志“逆向利用”

日志来源：Mihomo stdout/stderr 或 Controller 日志接口。

1. 后端接收日志流，进行缓存。
2. 前端展示日志。
3. 当用户在 UI 发现被阻断的或高延迟的连接时，点击**快捷建规则**。
4. 系统将该规则写入 `routing_rules`，并立即重新生成 `runtime.yaml`，触发**热更新**。新规则瞬间生效。

## 10. 流量与连接提取

- `GET /api/traffic` 和 `GET /api/connections` 直接轮询 controller API，将 Clash 视为黑盒数据源。
- 后端额外会有个后台任务去定时聚合这些数据，写入 `traffic_snapshots` 用于画历史曲线。

## 11. 本地订阅与节点管理

**R-Clash 是一个功能自治的本地客户端，系统后端全权负责订阅。**

流程如下：
1. 后端定时或手动请求订阅 URL 获取配置。
2. 后端负责解析协议文本，应用 `subscription_rules` （节点精选规则）和 `global_filter_rules`（全局规则）过滤掉劣质或不需要的节点。
3. 干净的节点入库到 `proxy_items` (kind='node')。
4. 重新计算所有依赖该订阅的 `proxy_group_members` 缓存。
5. 重新生成 `runtime.yaml`，并触发热更新。

## 12. 平台交互

Linux：
- 后端二进制直接管理 Mihomo 进程。

Windows/macOS/Android：
- Tauri 应用负责桌面和移动集成。
- 共享库作为 Rust 后端层，管理 Mihomo 进程并调用 controller。

## 13. 错误处理

后端统一把 controller 和进程错误转换成应用错误，并写入 `log_entries`：
- Mihomo 二进制不存在、配置生成失败、端口被占用、TUN 权限不足等。

## 14. 后端模块划分

按照“系统主导”的思想，在 `crates/rweb-clash` 共享库中的核心模块划分：

- `core`：Mihomo 进程管理、controller 客户端。
- `config`：系统配置、运行时 YAML 生成器（编译器）。
- `subscription`：本地订阅下载、解析、按规则清洗过滤。
- `asset`：统一出站对象池（对应 `proxy_items` 表），以及地区和协议增强分析。
- `proxy`：分组计算引擎、代理组策略拓扑、测速、节点选择的 API 劫持。
- `rule`：本地路由规则维护、规则集下载托管和合并。
- `log`：日志流采集和逆向解析建规则。
- `traffic`：高频流量和连接状态提取。
