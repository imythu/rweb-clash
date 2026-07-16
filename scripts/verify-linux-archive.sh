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
  "rweb-clash.service"
  "README.md"
  "LINUX.md"
  "release-smoke.sh"
  "release-smoke.ps1"
)

for file in "${required_files[@]}"; do
  if [[ ! -f "$release_dir/$file" ]]; then
    echo "archive is missing required file: $file" >&2
    exit 1
  fi
done

for executable in rweb-clash install.sh release-smoke.sh; do
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
if ! grep -q '^## 升级$' "$release_dir/LINUX.md"; then
  echo "Linux release documentation does not describe upgrades" >&2
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

echo "Linux archive contents and upgrade installation path verified: $archive"
