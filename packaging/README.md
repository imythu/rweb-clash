# 打包流程设计

本文档定义 rweb-clash 的发布打包流程。目标是让所有平台都拿到同一套前端和后端能力，同时按平台处理 Mihomo core、规则集缓存、GeoIP 运行时资源和系统集成差异。

## 目标平台

| 平台 | 形态 | 打包入口 | 主要产物 |
| --- | --- | --- | --- |
| macOS arm64 | Tauri 桌面应用 | `apps/desktop/src-tauri` | `.dmg` / `.app` |
| Windows amd64 | Tauri 桌面应用 | `apps/desktop/src-tauri` | `.msi` / `.exe` |
| Linux amd64 | 后端 + 前端静态单二进制 | `crates/rweb-clash-bin` | `.bin` / `.tar.gz` / Docker 镜像 |
| Linux arm64 | 后端 + 前端静态单二进制 | `crates/rweb-clash-bin` | `.bin` / `.tar.gz` / Docker 镜像 |

## 发布原则

1. 前端只构建一次，输出 `web/dist`。
2. 后端业务逻辑只维护在 `crates/rweb-clash`。
3. Mihomo core 不进 Git，打包时下载并写入平台产物。
4. 默认规则集不进数据库硬编码，打包时下载为初始缓存，首次运行后由应用现有刷新机制接管。
5. `geoip.metadb` 在打包时从上游最新 GitHub Release 获取，并按 Release API 返回的大小和 SHA256 digest 校验。
6. 可联网的构建使用“下载并校验”；不可联网的构建必须使用预填充缓存目录。
7. 运行时数据必须进入用户数据目录，不能写入安装目录。

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
    runtime-assets.toml
  cache/
    cores/
    rule-sets/
    runtime/
```

## 总流程

1. 清理上次发布缓存和产物。
2. 下载目标平台 Mihomo core 到 `packaging/cache/cores/<target>/`。
3. 读取 `packaging/manifests/rule-sets.toml`，下载默认规则集到 `packaging/cache/rule-sets/`。
4. 读取 `packaging/manifests/runtime-assets.toml`，解析上游最新 GitHub Release，下载并校验 `geoip.metadb`。
5. 执行 `pnpm --dir web install --frozen-lockfile` 和 `pnpm --dir web build`。
6. 按目标平台构建：
   - macOS/Windows：复制 core、默认规则集和 GeoIP 资源到 Tauri resource，再执行 Tauri build。
   - Linux：用 musl 静态目标把 `web/dist`、core、默认规则集和 GeoIP 资源嵌入 `rweb-clash-bin`。

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

脚本先读取对应 GitHub Release 的 asset 元数据，要求唯一匹配目标压缩包，并在解压前核对 asset ID、精确字节数和 GitHub 提供的 SHA256 digest。`manifest.json` 同时记录 release/asset 来源、压缩包摘要和解压后二进制摘要。shell 版本需要 `curl`、`jq`，以及 `sha256sum` 或 `shasum`。

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

清单中的 URL 继续使用可变的 `@release` 分支，构建开始时先通过 GitHub API 将该分支解析为一个 40 位 commit SHA，再用这个 SHA 下载全部 13 个文件。这样每次构建仍会获取最新 release，同时单次构建内所有平台和规则文件都固定到同一提交；原始 URL、解析后的 URL、commit、大小与 SHA256 都记录在 manifest 中。shell 版本需要 `curl`、`jq`，以及 `sha256sum` 或 `shasum`。

## GeoIP 运行时资源

脚本：

```text
scripts/package-runtime-assets.ps1
scripts/package-runtime-assets.sh
```

输出：

```text
packaging/cache/runtime/geoip.metadb
packaging/cache/runtime/manifest.json
```

脚本通过 GitHub latest release API 找到唯一的 `geoip.metadb` asset，先保存该次构建解析到的 release/asset 元数据，再按 API 提供的精确字节数和 SHA256 digest 校验下载内容。文件与 manifest 都通过同目录临时文件原子替换，失败时不会留下半个资源。

这项校验可以发现传输损坏和 API 元数据与下载内容不一致，但不是独立的发布者签名：上游仓库或 GitHub 账户被攻破时，攻击者仍可能同时替换 asset 和 digest。`packaging/cache/runtime/manifest.json` 会记录每次构建实际使用的 release ID、asset ID、时间、大小和 SHA256，供产物追踪与复核。shell 脚本需要 `curl`、`jq`，以及 `sha256sum` 或 `shasum`。

## Tauri 打包

Tauri 覆盖 macOS arm64 和 Windows amd64。Linux 不使用 Tauri。resources 包含 Mihomo core、默认规则集与 GeoIP 运行时资源；Windows 还包含 `resources/windows/rweb-clash-windows-helper.exe`。Windows 首次开启 TUN 时通过 UAC 安装特权服务，主桌面进程之后以普通权限运行。

构建步骤：

1. 构建 `web/dist`。
2. 将当前 target 的 Mihomo core 放入 Tauri resources：
   - `resources/core/mihomo` 或 `resources/core/mihomo.exe`
3. Windows 额外构建并放入 TUN helper：
   - `resources/windows/rweb-clash-windows-helper.exe`
4. 将默认规则集放入 Tauri resources：
   - `resources/rule-sets/*.list`
   - `resources/rule-sets/manifest.json`
5. 将已校验的 GeoIP 数据放入 Tauri resources：
   - `resources/runtime/geoip.metadb`
   - `resources/runtime/manifest.json`
6. Tauri 启动时解析 resource dir，初始化 `AppOptions.root_dir` 到平台 app data dir。
7. 首次启动复制 core、规则集和 GeoIP 数据到 app data dir。

## Linux 单二进制

Linux 目标是不依赖 Tauri，产出可跨常见发行版运行的 musl 静态可执行文件，同时提供后端 API 和前端静态资源。Release 同时发布独立 `.bin` 和包含 systemd 安装器的 `.tar.gz`。

构建步骤：

1. 构建 `web/dist`。
2. 准备 `packaging/cache/cores/linux-amd64/mihomo` 或 `packaging/cache/cores/linux-arm64/mihomo`。
3. 准备 `packaging/cache/rule-sets/`。
4. 准备 `packaging/cache/runtime/geoip.metadb` 与 manifest。
5. `cross build --release --locked --target <arch>-unknown-linux-musl -p rweb-clash-bin --features embedded-assets`。
6. 启动时释放内置资源到 root_dir。

服务器或 Docker 宿主使用压缩包内的 `install-systemd.sh` 安装 system service；桌面 Linux 可继续使用 `install.sh` 安装 user service。详细约束见 [`linux/README.md`](linux/README.md)。
