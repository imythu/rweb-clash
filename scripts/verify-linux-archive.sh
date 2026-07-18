#!/usr/bin/env bash
set -euo pipefail

archive=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --archive|-Archive)
      archive="$2"
      shift 2
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$archive" ]]; then
  echo "usage: scripts/verify-linux-archive.sh --archive dist/rweb-clash-linux-amd64.tar.gz" >&2
  exit 1
fi
if [[ ! -f "$archive" ]]; then
  echo "archive not found: $archive" >&2
  exit 1
fi

workdir="$(mktemp -d)"
cleanup() {
  rm -rf "$workdir"
}
trap cleanup EXIT

tar -xzf "$archive" -C "$workdir"

release_dir="$(find "$workdir" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
if [[ -z "$release_dir" ]]; then
  echo "archive does not contain a top-level release directory: $archive" >&2
  exit 1
fi

required_files=(
  "rweb-clash"
  "install.sh"
  "install-systemd.sh"
  "rweb-clash.service"
  "rweb-clash-system.service"
  "rweb-clash-ready.service"
  "rweb-clash.env"
  "README.md"
  "LINUX.md"
  "LICENSE"
  "release-smoke.sh"
  "release-smoke.ps1"
)

for file in "${required_files[@]}"; do
  if [[ ! -f "$release_dir/$file" ]]; then
    echo "archive is missing required file: $file" >&2
    exit 1
  fi
done

for executable in rweb-clash install.sh install-systemd.sh release-smoke.sh; do
  if [[ ! -x "$release_dir/$executable" ]]; then
    echo "archive file is not executable: $executable" >&2
    exit 1
  fi
done

service_file="$release_dir/rweb-clash.service"
if ! grep -q 'ExecStart=%h/.local/bin/rweb-clash' "$service_file"; then
  echo "systemd service does not start the installed rweb-clash binary" >&2
  exit 1
fi
if ! grep -q -- '--data-dir %h/.local/share/rweb-clash' "$service_file"; then
  echo "systemd service does not use the expected user data directory" >&2
  exit 1
fi
if ! grep -q '^Restart=on-failure$' "$service_file"; then
  echo "systemd service does not restart on failure" >&2
  exit 1
fi

system_service_file="$release_dir/rweb-clash-system.service"
for expected in \
  'User=rweb-clash' \
  'ExecStart=/usr/local/bin/rweb-clash --no-open' \
  'ExecStartPost=/usr/local/bin/rweb-clash --wait-api 3600' \
  'Before=rweb-clash-ready.service' \
  'WantedBy=multi-user.target' \
  'CapabilityBoundingSet=CAP_NET_ADMIN' \
  'AmbientCapabilities=CAP_NET_ADMIN' \
  'NoNewPrivileges=true'; do
  if ! grep -Fqx -- "$expected" "$system_service_file"; then
    echo "system systemd service is missing: $expected" >&2
    exit 1
  fi
done
if [[ $(grep -c '^CapabilityBoundingSet=' "$system_service_file") -ne 1 ]] ||
   [[ $(grep -c '^AmbientCapabilities=' "$system_service_file") -ne 1 ]]; then
  echo "system service must grant exactly one minimal capability set" >&2
  exit 1
fi

ready_service_file="$release_dir/rweb-clash-ready.service"
for expected in \
  'Requires=rweb-clash.service' \
  'After=rweb-clash.service' \
  'Before=docker.service containerd.service' \
  'ExecStart=/usr/local/bin/rweb-clash --wait-ready 3600' \
  'RequiredBy=docker.service containerd.service'; do
  if ! grep -Fqx -- "$expected" "$ready_service_file"; then
    echo "systemd readiness gate is missing: $expected" >&2
    exit 1
  fi
done
if grep -Eq '^(CapabilityBoundingSet|AmbientCapabilities)=' "$ready_service_file"; then
  echo "systemd readiness gate must not receive network administration capabilities" >&2
  exit 1
fi

install_script="$release_dir/install.sh"
if ! grep -q 'mktemp "${install_dir}/.rweb-clash.XXXXXX"' "$install_script"; then
  echo "Linux installer does not stage the binary in the install directory" >&2
  exit 1
fi
if ! grep -q 'mv -f "$temp_binary" "$install_dir/rweb-clash"' "$install_script"; then
  echo "Linux installer does not atomically replace the installed binary" >&2
  exit 1
fi
if ! grep -q 'systemctl --user restart rweb-clash.service' "$install_script"; then
  echo "Linux installer does not restart the service during upgrades" >&2
  exit 1
fi
if ! grep -q '^## systemd 系统服务升级与卸载$' "$release_dir/LINUX.md"; then
  echo "Linux release documentation does not describe system service upgrades" >&2
  exit 1
fi

fake_home="$workdir/fake-home"
fake_bin="$workdir/fake-bin"
systemctl_log="$workdir/systemctl.log"
mkdir -p "$fake_home" "$fake_bin"
cat > "$fake_bin/systemctl" <<'SH'
#!/usr/bin/env sh
printf '%s\n' "$*" >> "$SYSTEMCTL_LOG"
SH
chmod +x "$fake_bin/systemctl"
HOME="$fake_home" SYSTEMCTL_LOG="$systemctl_log" PATH="$fake_bin:$PATH" "$install_script" >/dev/null
cmp "$release_dir/rweb-clash" "$fake_home/.local/bin/rweb-clash"
for expected in \
  '--user daemon-reload' \
  '--user enable rweb-clash.service' \
  '--user restart rweb-clash.service'; do
  if ! grep -Fqx -- "$expected" "$systemctl_log"; then
    echo "Linux installer did not invoke systemctl as expected: $expected" >&2
    exit 1
  fi
done

system_root="$workdir/system-root"
DESTDIR="$system_root" "$release_dir/install-systemd.sh" >/dev/null
cmp "$release_dir/rweb-clash" "$system_root/usr/local/bin/rweb-clash"
cmp "$release_dir/rweb-clash-system.service" "$system_root/etc/systemd/system/rweb-clash.service"
cmp "$release_dir/rweb-clash-ready.service" "$system_root/etc/systemd/system/rweb-clash-ready.service"
cmp "$release_dir/rweb-clash.env" "$system_root/etc/default/rweb-clash"
if [[ ! -d "$system_root/var/lib/rweb-clash" ]]; then
  echo "system installer did not create the service data directory" >&2
  exit 1
fi
if command -v systemd-analyze >/dev/null 2>&1; then
  printf 'rweb-clash:x:991:991::/var/lib/rweb-clash:/usr/sbin/nologin\n' > "$system_root/etc/passwd"
  printf 'rweb-clash:x:991:\n' > "$system_root/etc/group"
  printf '[Unit]\nDescription=System Initialization\nDefaultDependencies=no\n' \
    > "$system_root/etc/systemd/system/sysinit.target"
  systemd-analyze verify --root="$system_root" /etc/systemd/system/rweb-clash.service
  systemd-analyze verify --root="$system_root" /etc/systemd/system/rweb-clash-ready.service
  systemctl --root="$system_root" enable rweb-clash.service rweb-clash-ready.service >/dev/null
  if [[ ! -L "$system_root/etc/systemd/system/multi-user.target.wants/rweb-clash.service" ]]; then
    echo "system service is not enabled for multi-user.target" >&2
    exit 1
  fi
  for dependency in docker.service containerd.service; do
    if [[ ! -L "$system_root/etc/systemd/system/$dependency.requires/rweb-clash-ready.service" ]]; then
      echo "readiness gate is not required by $dependency" >&2
      exit 1
    fi
  done
  DESTDIR="$system_root" "$release_dir/install-systemd.sh" uninstall >/dev/null
  for removed in \
    usr/local/bin/rweb-clash \
    etc/systemd/system/rweb-clash.service \
    etc/systemd/system/rweb-clash-ready.service \
    etc/systemd/system/multi-user.target.wants/rweb-clash.service \
    etc/systemd/system/docker.service.requires/rweb-clash-ready.service \
    etc/systemd/system/containerd.service.requires/rweb-clash-ready.service; do
    if [[ -e "$system_root/$removed" || -L "$system_root/$removed" ]]; then
      echo "staged system uninstall did not remove: $removed" >&2
      exit 1
    fi
  done
  if [[ ! -f "$system_root/etc/default/rweb-clash" || ! -d "$system_root/var/lib/rweb-clash" ]]; then
    echo "staged system uninstall did not preserve configuration and data" >&2
    exit 1
  fi
fi
system_install_script="$release_dir/install-systemd.sh"
for expected in \
  'systemctl enable rweb-clash.service' \
  'systemctl enable rweb-clash-ready.service' \
  'systemctl restart rweb-clash.service' \
  'useradd --system' \
  'chown "$service_user:$service_group" "$data_dir"' \
  'journalctl -u rweb-clash.service'; do
  if ! grep -Fq -- "$expected" "$system_install_script"; then
    echo "system installer is missing required behavior: $expected" >&2
    exit 1
  fi
done
if grep -Fq 'chown -R' "$system_install_script"; then
  echo "system installer must not recursively chown service-controlled data" >&2
  exit 1
fi
if ! grep -Fq 'RWEB_CLASH_API_TOKEN=%s' "$system_install_script"; then
  echo "system installer does not generate a local API token" >&2
  exit 1
fi
if ! grep -Fq 'modprobe tun' "$system_install_script" ||
   ! grep -Fq 'TUN support is ready' "$system_install_script"; then
  echo "system installer does not prepare and report host TUN device support" >&2
  exit 1
fi

mock_install_script="$workdir/install-systemd-mock.sh"
sed \
  -e '1,/^  if \[ -n "\$destdir" \]; then$/s/^  if \[ -n "\$destdir" \]; then$/  if false; then/' \
  -e 's#^    /etc/systemd/system/#    ${destdir}/etc/systemd/system/#' \
  -e 's#^  rm -f /etc/systemd/system/#  rm -f ${destdir}/etc/systemd/system/#' \
  "$system_install_script" > "$mock_install_script"
chmod +x "$mock_install_script"
bash -n "$mock_install_script"

cat > "$fake_bin/systemctl" <<'SH'
#!/usr/bin/env sh
printf '%s\n' "$*" >> "$SYSTEMCTL_LOG"
case "$1" in
  disable) exit 1 ;;
  is-active) exit 3 ;;
  stop)
    if [ "${FAIL_STOP:-0}" = "1" ]; then
      exit 1
    fi
    ;;
esac
exit 0
SH
chmod +x "$fake_bin/systemctl"

stage_mock_system_install() {
  local root="$1"
  install -d -m 0755 \
    "$root/usr/local/bin" \
    "$root/etc/systemd/system/multi-user.target.wants" \
    "$root/etc/systemd/system/docker.service.requires" \
    "$root/etc/systemd/system/containerd.service.requires" \
    "$root/etc/default" \
    "$root/var/lib/rweb-clash"
  install -m 0755 "$release_dir/rweb-clash" "$root/usr/local/bin/rweb-clash"
  install -m 0644 "$release_dir/rweb-clash-system.service" "$root/etc/systemd/system/rweb-clash.service"
  install -m 0644 "$release_dir/rweb-clash-ready.service" "$root/etc/systemd/system/rweb-clash-ready.service"
  install -m 0640 "$release_dir/rweb-clash.env" "$root/etc/default/rweb-clash"
  ln -s ../rweb-clash.service "$root/etc/systemd/system/multi-user.target.wants/rweb-clash.service"
  ln -s ../rweb-clash-ready.service "$root/etc/systemd/system/docker.service.requires/rweb-clash-ready.service"
  ln -s ../rweb-clash-ready.service "$root/etc/systemd/system/containerd.service.requires/rweb-clash-ready.service"
}

mock_uninstall_root="$workdir/mock-uninstall-root"
stage_mock_system_install "$mock_uninstall_root"
: > "$systemctl_log"
DESTDIR="$mock_uninstall_root" SYSTEMCTL_LOG="$systemctl_log" PATH="$fake_bin:$PATH" \
  "$mock_install_script" uninstall >/dev/null
for expected in \
  'stop rweb-clash-ready.service' \
  'stop rweb-clash.service'; do
  if ! grep -Fqx -- "$expected" "$systemctl_log"; then
    echo "system uninstall skipped an installed or activating service: $expected" >&2
    exit 1
  fi
done
if grep -Fq 'is-active' "$systemctl_log"; then
  echo "system uninstall must not skip activating services via systemctl is-active" >&2
  exit 1
fi

failed_uninstall_root="$workdir/failed-uninstall-root"
stage_mock_system_install "$failed_uninstall_root"
if DESTDIR="$failed_uninstall_root" SYSTEMCTL_LOG="$systemctl_log" FAIL_STOP=1 PATH="$fake_bin:$PATH" \
  "$mock_install_script" uninstall >/dev/null 2>&1; then
  echo "system uninstall succeeded after systemctl stop failed" >&2
  exit 1
fi
if [[ ! -f "$failed_uninstall_root/etc/systemd/system/rweb-clash-ready.service" ||
      ! -f "$failed_uninstall_root/etc/systemd/system/rweb-clash.service" ]]; then
  echo "system uninstall removed unit files after systemctl stop failed" >&2
  exit 1
fi

echo "Linux archive contents and upgrade installation path verified: $archive"
