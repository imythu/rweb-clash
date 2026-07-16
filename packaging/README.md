# 打包流程设计

本文档定义 rweb-clash 的发布打包流程。目标是让所有平台都拿到同一套前端和后端能力，同时按平台处理 Mihomo core、规则集缓存和系统集成差异。

## 目标平台

| 平台 | 形态 | 打包入口 | 主要产物 |
| --- | --- | --- | --- |
| macOS arm64 | Tauri 桌面应用 | `apps/desktop/src-tauri` | `.dmg` / `.app` |
| Windows amd64 | Tauri 桌面应用 | `apps/desktop/src-tauri` | `.msi` / `.exe` |
| Linux amd64 | 后端 + 前端单二进制 | `crates/rweb-clash-bin` | `rweb-clash` / Docker 镜像 |
| Linux arm64 | 后端 + 前端单二进制 | `crates/rweb-clash-bin` | `rweb-clash` / Docker 镜像 |

## 发布原则

1. 前端只构建一次，输出 `web/dist`。
2. 后端业务逻辑只维护在 `crates/rweb-clash`。
3. Mihomo core 不进 Git，打包时下载并写入平台产物。
4. 默认规则集不进数据库硬编码，打包时下载为初始缓存，首次运行后由应用现有刷新机制接管。
5. 可联网的构建使用“下载并校验”；不可联网的构建必须使用预填充缓存目录。
6. 运行时数据必须进入用户数据目录，不能写入安装目录。

## 目录约定

```text
packaging/
  README.md
  tauri/
    README.md
  linux/
    README.md
  manifests/
    cores.toml
    rule-sets.toml
  cache/
    cores/
    rule-sets/
```

## 总流程

1. 清理上次发布缓存和产物。
2. 下载目标平台 Mihomo core 到 `packaging/cache/cores/<target>/`。
3. 读取 `packaging/manifests/rule-sets.toml`，下载默认规则集到 `packaging/cache/rule-sets/`。
4. 执行 `pnpm --dir web install --frozen-lockfile` 和 `pnpm --dir web build`。
5. 按目标平台构建：
   - macOS/Windows：复制 core 和默认规则集到 Tauri resource，再执行 Tauri build。
   - Linux：把 `web/dist`、core、默认规则集作为编译期资源嵌入 `rweb-clash-bin`。

GitHub Release 前必须按 [`release-checklist.md`](release-checklist.md) 核对目标平台、产物和 Docker manifest。

## Core 下载

脚本：

```text
scripts/package-core.ps1
scripts/package-core.sh
```

示例：

```text
scripts/package-core.sh --target linux-amd64 --version latest
```

输出：

```text
packaging/cache/cores/windows-amd64/mihomo.exe
packaging/cache/cores/macos-arm64/mihomo
packaging/cache/cores/linux-amd64/mihomo
packaging/cache/cores/linux-arm64/mihomo
```

## 规则集下载

脚本：

```text
scripts/package-rule-sets.ps1
scripts/package-rule-sets.sh
```

输出：

```text
packaging/cache/rule-sets/<id>.list
packaging/cache/rule-sets/manifest.json
```

## Tauri 打包

Tauri 覆盖 macOS arm64 和 Windows amd64。Linux 不使用 Tauri。resources 包含 Mihomo core 与默认规则集。

构建步骤：

1. 构建 `web/dist`。
2. 将当前 target 的 Mihomo core 放入 Tauri resources：
   - `resources/core/mihomo` 或 `resources/core/mihomo.exe`
3. 将默认规则集放入 Tauri resources：
   - `resources/rule-sets/*.list`
   - `resources/rule-sets/manifest.json`
4. Tauri 启动时解析 resource dir，初始化 `AppOptions.root_dir` 到平台 app data dir。
5. 首次启动复制 core 和规则集到 app data dir。

## Linux 单二进制

Linux 目标是不依赖 Tauri，产出一个可执行文件，同时提供后端 API 和前端静态资源。

构建步骤：

1. 构建 `web/dist`。
2. 准备 `packaging/cache/cores/linux-amd64/mihomo` 或 `packaging/cache/cores/linux-arm64/mihomo`。
3. 准备 `packaging/cache/rule-sets/`。
4. `cargo build --release -p rweb-clash-bin --features embedded-assets`。
5. 启动时释放内置资源到 root_dir。
