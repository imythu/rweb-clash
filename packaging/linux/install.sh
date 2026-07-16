#!/usr/bin/env sh
set -eu

install_dir="${HOME}/.local/bin"
service_dir="${HOME}/.config/systemd/user"
data_dir="${HOME}/.local/share/rweb-clash"
listen="${RWEB_CLASH_LISTEN:-127.0.0.1:31990}"

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
binary="${script_dir}/rweb-clash"
service="${script_dir}/rweb-clash.service"

if [ ! -f "$binary" ]; then
  echo "rweb-clash binary not found next to install.sh" >&2
  exit 1
fi

mkdir -p "$install_dir" "$service_dir" "$data_dir"
temp_binary="$(mktemp "${install_dir}/.rweb-clash.XXXXXX")"
cleanup() {
  if [ -n "${temp_binary:-}" ]; then
    rm -f "$temp_binary"
  fi
}
trap cleanup EXIT HUP INT TERM

cp "$binary" "$temp_binary"
chmod 0755 "$temp_binary"
mv -f "$temp_binary" "$install_dir/rweb-clash"
temp_binary=""

if command -v systemctl >/dev/null 2>&1 && [ -f "$service" ]; then
  cp "$service" "$service_dir/rweb-clash.service"
  if systemctl --user daemon-reload \
    && systemctl --user enable rweb-clash.service \
    && systemctl --user restart rweb-clash.service; then
    echo "Installed or upgraded and restarted rweb-clash user service."
  else
    echo "Installed rweb-clash to $install_dir/rweb-clash."
    echo "systemd user service could not be started in this session; start it manually:"
    echo "  $install_dir/rweb-clash --listen $listen --data-dir $data_dir"
  fi
else
  echo "Installed rweb-clash to $install_dir/rweb-clash."
  echo "systemctl --user is unavailable; start it manually:"
  echo "  $install_dir/rweb-clash --listen $listen --data-dir $data_dir"
fi

echo "Open http://$listen"
