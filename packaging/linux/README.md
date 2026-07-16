# Linux 打包

Linux 目标是后端 + 前端一起打包的单二进制发布，并发布 Docker 镜像。完整流程见 [`../README.md`](../README.md)。

## 打包职责

1. 构建 `web/dist`。
2. 下载并校验 Linux amd64 或 Linux arm64 的 Mihomo core。
3. 从上游最新 Release 下载并校验 `geoip.metadb`。
4. 从各规则源的 `@release` 分支下载并校验 13 个默认规则集。
5. 构建 `rweb-clash-bin`，把前端、core、GeoIP 和默认规则集作为内置资源。
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

## 快速安装

GitHub Release 的 Linux 压缩包内包含 `install.sh`。解压后执行：

```text
./install.sh
```

脚本会把 `rweb-clash` 安装到 `~/.local/bin/`，创建 `~/.local/share/rweb-clash`，并在支持 systemd user service 的系统上启用 `rweb-clash.service`。安装完成后打开：

```text
http://127.0.0.1:31990
```

## 升级

下载新版本的 Linux 压缩包和同名 `.sha256` 文件，在同一目录先校验再解压：

```text
sha256sum -c rweb-clash-linux-amd64.tar.gz.sha256
tar -xzf rweb-clash-linux-amd64.tar.gz
cd rweb-clash-linux-amd64
./install.sh
```

arm64 使用对应的 `rweb-clash-linux-arm64` 文件名。`install.sh` 会在安装目录内写入临时文件后原子替换二进制，并显式重启已有的 systemd user service；`~/.local/share/rweb-clash` 中的数据和配置会保留。

## systemd 用户服务

仓库提供 `packaging/linux/rweb-clash.service` 模板。安装到用户服务：

```text
mkdir -p ~/.config/systemd/user ~/.local/bin
cp rweb-clash ~/.local/bin/
cp packaging/linux/rweb-clash.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now rweb-clash.service
```
