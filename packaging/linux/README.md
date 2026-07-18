# Linux 打包

Linux 目标是后端 + 前端一起打包的静态单二进制发布，并发布 Docker 镜像。完整流程见 [`../README.md`](../README.md)。

## 打包职责

1. 构建 `web/dist`。
2. 下载并校验 Linux amd64 或 Linux arm64 的 Mihomo core。
3. 从上游最新 Release 下载并校验 `geoip.metadb`。
4. 从各规则源的 `@release` 分支下载并校验 13 个默认规则集。
5. 使用 musl 构建 amd64/arm64 `rweb-clash-bin`，把前端、core、GeoIP 和默认规则集作为内置资源。
6. 首次启动时释放资源到运行时 root dir；已有的有效 GeoIP 和规则缓存会保留。
7. Axum 同时提供 API、静态前端和 SPA fallback。

## Docker

GitHub Actions 使用仓库根目录的 `Dockerfile` 构建并发布 `linux/amd64` 和 `linux/arm64` 多架构镜像到 GHCR。容器内部监听 `0.0.0.0:31990`，数据目录为 `/var/lib/rweb-clash`；宿主机端口默认只绑定到 `127.0.0.1`。

非回环监听必须设置至少 16 字符的 `RWEB_CLASH_API_TOKEN`，API 使用 Bearer 令牌鉴权。即使启用令牌，也不要把明文 HTTP 直接暴露到公网；远程访问应通过可信的 TLS 反向代理和网络访问控制。

本地构建示例：

```text
docker buildx build --platform linux/amd64,linux/arm64 -t ghcr.io/OWNER/REPO:latest --push .
```

运行示例：

```text
export RWEB_CLASH_API_TOKEN="$(openssl rand -hex 32)"
docker run -d --name rweb-clash --restart unless-stopped \
  -e RWEB_CLASH_API_TOKEN \
  -p 127.0.0.1:31990:31990 \
  -v rweb-clash-data:/var/lib/rweb-clash \
  --cap-drop ALL \
  --security-opt no-new-privileges \
  ghcr.io/OWNER/REPO:latest
```

默认容器以非 root 用户运行，并且不授予 Linux capabilities。只有启用 TUN 时才需要额外权限；Compose 可显式叠加 TUN 配置：

```text
docker compose -f docker-compose.yml -f docker-compose.tun.yml up -d
```

直接运行镜像时，对应参数为 `--cap-add NET_ADMIN --device /dev/net/tun:/dev/net/tun`。

从旧版 root 容器升级时，已有卷可能仍由 root 持有。首次启动新版前执行一次权限迁移：

```text
docker run --rm --user 0:0 \
  -v rweb-clash-data:/var/lib/rweb-clash \
  --entrypoint chown ghcr.io/OWNER/REPO:latest \
  -R 10001:10001 /var/lib/rweb-clash
```

如果使用宿主机 bind mount，则在宿主机上将对应数据目录递归调整为 UID/GID `10001:10001`。

首次打开 `http://127.0.0.1:31990/#token=<RWEB_CLASH_API_TOKEN>`。前端会在本机保存令牌并立即从地址栏移除它；也可以在解锁页中直接输入。

## 运行参数

```text
RWEB_CLASH_LISTEN=127.0.0.1:31990
RWEB_CLASH_ROOT=/opt/rweb-clash-data
RWEB_CLASH_LOG=info
RWEB_CLASH_API_TOKEN=<至少 16 字符；非回环监听必填>
RWEB_CLASH_ALLOWED_ORIGINS=<额外允许的跨域 Origin，逗号分隔>
RWEB_CLASH_ALLOW_PRIVATE_SOURCES=0
RWEB_CLASH_MIHOMO_VALIDATION_TIMEOUT_SECS=120
```

Linux 单二进制也支持 CLI 参数：

```text
rweb-clash --listen 127.0.0.1:31990 --data-dir ~/.local/share/rweb-clash --log-level info
```

发布包内包含 smoke test 脚本。发布验收时加上 `--verify-embedded-assets`，会在断言 core、GeoIP 和 13 个规则文件均已释放后实际启动并停止 Mihomo：

```text
./release-smoke.sh --verify-embedded-assets --binary ./rweb-clash --listen 127.0.0.1:32990
```

## systemd 系统服务安装（服务器和 Docker 宿主推荐）

先校验并解压 GitHub Release：

```text
sha256sum -c rweb-clash-linux-amd64.tar.gz.sha256
tar -xzf rweb-clash-linux-amd64.tar.gz
cd rweb-clash-linux-amd64
./install-systemd.sh
```

脚本在需要时通过 `sudo` 提权，并执行以下操作：

- 安装 `/usr/local/bin/rweb-clash`。
- 创建无登录权限的 `rweb-clash` 系统用户。
- 使用 `/var/lib/rweb-clash` 保存数据库、Mihomo 和规则资源。
- 安装并启用 `/etc/systemd/system/rweb-clash.service` 和容器启动就绪门禁 `rweb-clash-ready.service`。
- 保持服务账号为非 root，同时默认仅授予 TUN 和自动路由所需的 `CAP_NET_ADMIN`；其他 Linux capabilities 仍被 capability bounding set 排除。
- 在 API 真正可响应后才完成 systemd 启动流程。
- 在 `/etc/default/rweb-clash` 未配置令牌时生成 64 位十六进制 API token，避免其他本机用户控制系统代理服务。
- 将 readiness gate 作为 `docker.service` 和 `containerd.service` 的 Required 依赖并排在它们之前；以后单独启动或重启 Docker/Containerd 时，都会重新检查 rweb-clash，而不是复用一次过期的启动结果。

安装完成后打开：

```text
http://127.0.0.1:31990/#token=<安装器输出的 API token>
```

安装 systemd 服务不会暗中启动 Mihomo。添加订阅并验证可用后，需要在 Web UI 中显式开启“自动启动”，否则重启主机后只有管理服务会运行，Mihomo mixed port 不会监听。主 service 只以管理 API 可用作为启动成功条件，因此 core 配置错误时仍能打开 UI 修复；Docker/Containerd 每次启动都会运行一次 core-aware 的短生命周期 gate。该 gate 在 `auto_start=true` 时要求 core 真正达到 `running`，若 core 不可用，Required 依赖会阻止容器管理服务在无代理时继续启动；未启用 auto-start 时只要求管理 API 可用。检查状态：

```text
set -a; . /etc/default/rweb-clash; set +a
curl -fsS -H "Authorization: Bearer $RWEB_CLASH_API_TOKEN" http://127.0.0.1:31990/api/core/status
systemctl status rweb-clash.service
systemctl status rweb-clash-ready.service
journalctl -u rweb-clash.service -f
```

`rweb-clash-ready.service` 是依赖门禁，只应查看状态；不要手工 stop/restart 它，因为 systemd 的 `Requires` 关系会把该操作传播给 Docker/Containerd。日常管理使用 `rweb-clash.service`，修复 core 后重启实际依赖它的容器服务即可重新执行 gate。

Docker daemon 应代理到 Mihomo mixed port（默认 `127.0.0.1:7890`），不是 Web/API 端口 `31990`。例如 `/etc/systemd/system/docker.service.d/http-proxy.conf`：

```ini
[Service]
Environment="HTTP_PROXY=http://127.0.0.1:7890"
Environment="HTTPS_PROXY=http://127.0.0.1:7890"
Environment="NO_PROXY=localhost,127.0.0.1,::1"
```

修改后执行 `systemctl daemon-reload`。只在可以中断现有容器时再重启 Docker；安装器不会擅自重启 Docker。若在 UI 中修改了 mixed port，也必须同步修改 Docker 配置。

system service 使用独立账号，不具备桌面会话 D-Bus，因此不要在该模式下启用 GNOME“系统代理”开关。它面向 headless/Docker 宿主。默认 unit 只额外授予 `CAP_NET_ADMIN`，让 Mihomo 可以创建 TUN、写入路由并配置自动重定向；API、数据文件和进程仍由低权限 `rweb-clash` 用户持有，其他 capabilities 不会授予。若宿主没有 `/dev/net/tun`，先执行 `sudo modprobe tun` 并确认该字符设备存在。

## systemd 系统服务升级与卸载

升级仍先校验并解压新包，然后执行：

```text
./install-systemd.sh update
```

安装器会先完整 staging，再替换 root-owned 的二进制和两个 unit。新服务启动前失败会恢复旧 package 文件；新进程一旦运行过，数据库迁移可能已经发生，安装器不会冒险自动降级旧程序，而会保留 root-only 备份路径、输出 journal 并返回失败。启动等待期间被信号中断也会保留备份。`update` 会保留已有服务的 enabled/active 状态，安装器不会以 root 遍历或改写服务账号控制的运行资源；`/var/lib/rweb-clash` 中的数据、API token 及 `/etc/default/rweb-clash` 其他配置都会保留。

卸载服务和二进制但保留数据：

```text
./install-systemd.sh uninstall
```

## systemd 用户服务（Linux 桌面）

桌面 Linux 仍可使用不提权的用户安装器：

```text
./install.sh
```

它会安装到 `~/.local/bin/` 和 `~/.local/share/rweb-clash`，并启用 `systemctl --user` 服务。该模式依赖用户 systemd manager，不适合作为 Docker daemon 的开机代理依赖。

手动安装用户 unit：

仓库提供 `packaging/linux/rweb-clash.service` 模板。安装到用户服务：

```text
mkdir -p ~/.config/systemd/user ~/.local/bin
cp rweb-clash ~/.local/bin/
cp packaging/linux/rweb-clash.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now rweb-clash.service
```

## 独立二进制

Release 同时提供不含脚本外壳的静态文件：

```text
rweb-clash-linux-amd64.bin
rweb-clash-linux-amd64.bin.sha256
rweb-clash-linux-arm64.bin
rweb-clash-linux-arm64.bin.sha256
```

直接运行前先校验并添加执行权限：

```text
sha256sum -c rweb-clash-linux-amd64.bin.sha256
chmod +x rweb-clash-linux-amd64.bin
./rweb-clash-linux-amd64.bin --data-dir ./rweb-clash-data
```
