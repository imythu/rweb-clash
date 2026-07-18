#!/usr/bin/env sh
set -eu

usage() {
  cat <<'EOF'
Usage:
  ./install-systemd.sh [install|update]
  ./install-systemd.sh uninstall

The system service uses:
  /usr/local/bin/rweb-clash
  /etc/systemd/system/rweb-clash.service
  /etc/systemd/system/rweb-clash-ready.service
  /etc/default/rweb-clash
  /var/lib/rweb-clash

Uninstall preserves configuration and data in /etc/default/rweb-clash and
/var/lib/rweb-clash.
EOF
}

action="install"
if [ "$#" -gt 0 ]; then
  action="$1"
  shift
fi
if [ "$#" -ne 0 ]; then
  usage >&2
  exit 1
fi
case "$action" in
  install|update) ;;
  uninstall) ;;
  --help|-h|help)
    usage
    exit 0
    ;;
  *)
    echo "unknown action: $action" >&2
    usage >&2
    exit 1
    ;;
esac

if [ -z "${DESTDIR:-}" ] && [ "$(id -u)" -ne 0 ]; then
  if ! command -v sudo >/dev/null 2>&1; then
    echo "system installation requires root; rerun with sudo" >&2
    exit 1
  fi
  exec sudo -- "$0" "$action"
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
source_binary="${script_dir}/rweb-clash"
source_service="${script_dir}/rweb-clash-system.service"
source_ready_service="${script_dir}/rweb-clash-ready.service"
source_env="${script_dir}/rweb-clash.env"
destdir="${DESTDIR:-}"
binary_target="${destdir}/usr/local/bin/rweb-clash"
service_target="${destdir}/etc/systemd/system/rweb-clash.service"
ready_service_target="${destdir}/etc/systemd/system/rweb-clash-ready.service"
env_target="${destdir}/etc/default/rweb-clash"
data_dir="${destdir}/var/lib/rweb-clash"
core_target="${data_dir}/cache-core/mihomo"

if [ "$action" = "uninstall" ]; then
  if [ -n "$destdir" ]; then
    rm -f \
      "$binary_target" \
      "$service_target" \
      "$ready_service_target" \
      "${destdir}/etc/systemd/system/multi-user.target.wants/rweb-clash.service" \
      "${destdir}/etc/systemd/system/docker.service.requires/rweb-clash-ready.service" \
      "${destdir}/etc/systemd/system/containerd.service.requires/rweb-clash-ready.service"
    echo "Removed staged system service from $destdir"
    exit 0
  fi
  systemctl disable rweb-clash-ready.service >/dev/null 2>&1 || true
  rm -f \
    /etc/systemd/system/docker.service.requires/rweb-clash-ready.service \
    /etc/systemd/system/containerd.service.requires/rweb-clash-ready.service
  systemctl disable rweb-clash.service >/dev/null 2>&1 || true
  rm -f /etc/systemd/system/multi-user.target.wants/rweb-clash.service
  systemctl daemon-reload
  if [ -e "$ready_service_target" ]; then
    systemctl stop rweb-clash-ready.service
  fi
  if [ -e "$service_target" ]; then
    systemctl stop rweb-clash.service
  fi
  rm -f "$ready_service_target" "$service_target" "$binary_target"
  systemctl daemon-reload
  systemctl reset-failed rweb-clash.service >/dev/null 2>&1 || true
  systemctl reset-failed rweb-clash-ready.service >/dev/null 2>&1 || true
  echo "Removed rweb-clash system service and binary."
  echo "Preserved $env_target and $data_dir"
  exit 0
fi

for required in "$source_binary" "$source_service" "$source_ready_service" "$source_env"; do
  if [ ! -f "$required" ]; then
    echo "required release file not found: $required" >&2
    exit 1
  fi
done

if [ -n "$destdir" ]; then
  install -d -m 0755 "$(dirname -- "$binary_target")" "$(dirname -- "$service_target")" "$(dirname -- "$env_target")"
  install -d -m 0750 "$data_dir"
  install -m 0755 "$source_binary" "$binary_target"
  install -m 0644 "$source_service" "$service_target"
  install -m 0644 "$source_ready_service" "$ready_service_target"
  if [ ! -e "$env_target" ]; then
    install -m 0640 "$source_env" "$env_target"
  fi
  echo "Staged rweb-clash system service under $destdir"
  exit 0
fi

if ! command -v systemctl >/dev/null 2>&1; then
  echo "systemctl is required for system installation" >&2
  exit 1
fi
if ! "$source_binary" --help >/dev/null 2>&1; then
  echo "release binary cannot run on this host; check the downloaded architecture" >&2
  exit 1
fi

tun_device_ready=0
if [ -c /dev/net/tun ]; then
  tun_device_ready=1
elif command -v modprobe >/dev/null 2>&1; then
  if modprobe tun >/dev/null 2>&1 && command -v udevadm >/dev/null 2>&1; then
    udevadm settle --timeout=5 >/dev/null 2>&1 || true
  fi
  if [ -c /dev/net/tun ]; then
    tun_device_ready=1
  fi
fi

service_user="rweb-clash"
service_group="rweb-clash"
if ! getent group "$service_group" >/dev/null 2>&1; then
  groupadd --system "$service_group"
fi
if ! id -u "$service_user" >/dev/null 2>&1; then
  nologin="/usr/sbin/nologin"
  if [ ! -x "$nologin" ]; then
    nologin="/sbin/nologin"
  fi
  if [ ! -x "$nologin" ]; then
    nologin="/bin/false"
  fi
  useradd --system --gid "$service_group" --home-dir "$data_dir" --no-create-home --shell "$nologin" "$service_user"
fi

install -d -m 0755 /usr/local/bin /etc/systemd/system /etc/default
if [ -L "$data_dir" ]; then
  echo "refusing to use a symlink as the service data directory: $data_dir" >&2
  exit 1
fi
install -d -m 0750 -o "$service_user" -g "$service_group" "$data_dir"
chown "$service_user:$service_group" "$data_dir"
if [ ! -e "$env_target" ]; then
  install -m 0640 -o root -g "$service_group" "$source_env" "$env_target"
else
  chown root:"$service_group" "$env_target"
  chmod 0640 "$env_target"
fi

generated_api_token=""
if ! grep -Eq '^[[:space:]]*RWEB_CLASH_API_TOKEN[[:space:]]*=' "$env_target"; then
  if [ ! -r /dev/urandom ] || ! command -v od >/dev/null 2>&1 || ! command -v tr >/dev/null 2>&1; then
    echo "cannot generate a secure API token; /dev/urandom, od, and tr are required" >&2
    exit 1
  fi
  generated_api_token=$(LC_ALL=C od -An -N32 -tx1 /dev/urandom | tr -d ' \n')
  if [ "${#generated_api_token}" -ne 64 ]; then
    echo "failed to generate a secure API token" >&2
    exit 1
  fi
  printf '\nRWEB_CLASH_API_TOKEN=%s\n' "$generated_api_token" >> "$env_target"
fi

backup_dir=$(mktemp -d /var/tmp/rweb-clash-install.XXXXXX)
temp_binary=""
temp_service=""
temp_ready_service=""
had_binary=0
had_service=0
had_ready_service=0
was_enabled=0
was_ready_enabled=0
enable_ready_after_update=0
was_active=0
commit_started=0
start_attempted=0
install_completed=0
preserve_backup=0

restore_prestart_files() {
  set +e
  if [ "$had_binary" -eq 1 ]; then
    cp -p "$backup_dir/rweb-clash" "$binary_target"
  else
    rm -f "$binary_target"
  fi
  if [ "$had_service" -eq 1 ]; then
    cp -p "$backup_dir/rweb-clash.service" "$service_target"
  else
    rm -f "$service_target"
  fi
  if [ "$had_ready_service" -eq 1 ]; then
    cp -p "$backup_dir/rweb-clash-ready.service" "$ready_service_target"
  else
    rm -f "$ready_service_target"
  fi
  systemctl daemon-reload
  if [ "$was_enabled" -eq 1 ]; then
    systemctl enable rweb-clash.service >/dev/null
  else
    systemctl disable rweb-clash.service >/dev/null 2>&1
  fi
  if [ "$was_ready_enabled" -eq 1 ]; then
    systemctl enable rweb-clash-ready.service >/dev/null
  else
    systemctl disable rweb-clash-ready.service >/dev/null 2>&1
  fi
  if [ "$was_active" -eq 1 ] && [ "$had_binary" -eq 1 ] && [ "$had_service" -eq 1 ]; then
    systemctl restart rweb-clash.service >/dev/null 2>&1
  else
    systemctl stop rweb-clash.service >/dev/null 2>&1
  fi
  set -e
}

cleanup() {
  status=$?
  trap - EXIT HUP INT TERM
  if [ "$status" -ne 0 ] && [ "$commit_started" -eq 1 ] && [ "$start_attempted" -eq 0 ] && [ "$install_completed" -eq 0 ]; then
    echo "installation failed before service startup; restoring previous package files" >&2
    restore_prestart_files
  elif [ "$status" -ne 0 ] && [ "$start_attempted" -eq 1 ] && [ "$install_completed" -eq 0 ]; then
    preserve_backup=1
    echo "installation was interrupted after service startup began" >&2
    echo "previous package files are retained at $backup_dir" >&2
  fi
  [ -z "$temp_binary" ] || rm -f "$temp_binary"
  [ -z "$temp_service" ] || rm -f "$temp_service"
  [ -z "$temp_ready_service" ] || rm -f "$temp_ready_service"
  if [ "$preserve_backup" -eq 0 ]; then
    rm -rf "$backup_dir"
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

if [ -f "$binary_target" ]; then
  had_binary=1
  cp -p "$binary_target" "$backup_dir/rweb-clash"
fi
if [ -f "$service_target" ]; then
  had_service=1
  cp -p "$service_target" "$backup_dir/rweb-clash.service"
fi
if [ -f "$ready_service_target" ]; then
  had_ready_service=1
  cp -p "$ready_service_target" "$backup_dir/rweb-clash-ready.service"
fi
if systemctl is-enabled --quiet rweb-clash.service >/dev/null 2>&1; then
  was_enabled=1
fi
if systemctl is-enabled --quiet rweb-clash-ready.service >/dev/null 2>&1; then
  was_ready_enabled=1
fi
enable_ready_after_update=$was_ready_enabled
if [ "$had_ready_service" -eq 0 ] && [ "$was_enabled" -eq 1 ]; then
  enable_ready_after_update=1
fi
if systemctl is-active --quiet rweb-clash.service >/dev/null 2>&1; then
  was_active=1
fi

temp_binary=$(mktemp /usr/local/bin/.rweb-clash.XXXXXX)
install -m 0755 "$source_binary" "$temp_binary"
temp_service=$(mktemp /etc/systemd/system/.rweb-clash.service.XXXXXX)
install -m 0644 "$source_service" "$temp_service"
temp_ready_service=$(mktemp /etc/systemd/system/.rweb-clash-ready.service.XXXXXX)
install -m 0644 "$source_ready_service" "$temp_ready_service"

commit_started=1
mv -f "$temp_binary" "$binary_target"
temp_binary=""
mv -f "$temp_service" "$service_target"
temp_service=""
mv -f "$temp_ready_service" "$ready_service_target"
temp_ready_service=""

systemctl daemon-reload
if [ "$had_service" -eq 0 ] || [ "$action" = "install" ] || [ "$was_enabled" -eq 1 ]; then
  systemctl enable rweb-clash.service >/dev/null
else
  systemctl disable rweb-clash.service >/dev/null 2>&1
fi
if [ "$had_service" -eq 0 ] || [ "$action" = "install" ] || [ "$enable_ready_after_update" -eq 1 ]; then
  systemctl enable rweb-clash-ready.service >/dev/null
else
  systemctl disable rweb-clash-ready.service >/dev/null 2>&1
fi
if [ "$had_service" -eq 0 ] || [ "$action" = "install" ] || [ "$was_active" -eq 1 ]; then
  start_attempted=1
  preserve_backup=1
  if ! systemctl restart rweb-clash.service || ! systemctl is-active --quiet rweb-clash.service || [ ! -x "$core_target" ]; then
    install_completed=1
    preserve_backup=1
    echo "rweb-clash package files were installed, but the new service failed validation" >&2
    echo "database migrations may already have run; automatic binary rollback was not attempted" >&2
    echo "previous package files are retained at $backup_dir" >&2
    journalctl -u rweb-clash.service -n 30 --no-pager >&2 || true
    exit 1
  fi
  preserve_backup=0
else
  systemctl stop rweb-clash.service >/dev/null 2>&1 || true
fi
install_completed=1

if systemctl is-active --quiet rweb-clash.service; then
  echo "Installed and started rweb-clash as a system service."
else
  echo "Updated rweb-clash while preserving the inactive service state."
fi
if [ "$tun_device_ready" -eq 1 ]; then
  echo "TUN support is ready: CAP_NET_ADMIN is enabled and /dev/net/tun is available."
else
  echo "Warning: CAP_NET_ADMIN is enabled, but /dev/net/tun is unavailable on this host." >&2
  echo "Load the tun kernel module or ask the host provider to expose /dev/net/tun before enabling TUN mode." >&2
fi
if [ -n "$generated_api_token" ]; then
  echo "Generated API token: $generated_api_token"
  echo "Open once with token: http://127.0.0.1:31990/#token=$generated_api_token"
else
  echo "Open http://127.0.0.1:31990 with the API token from $env_target"
fi
echo "Logs: journalctl -u rweb-clash.service -f"
echo "For reboot-safe proxying, configure a subscription and explicitly enable core auto-start."
