#!/usr/bin/env bash
set -euo pipefail

target=""
version="latest"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --target|-Target)
      target="$2"
      shift 2
      ;;
    --version|-Version)
      version="$2"
      shift 2
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$target" ]]; then
  echo "usage: scripts/package-local.sh --target linux-amd64|linux-arm64|windows-amd64|macos-arm64 [--version latest|vX.Y.Z]" >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

package_linux_archive() {
  artifact="$1"
  rust_target="$2"
  binary="$repo_root/target/$rust_target/release/rweb-clash-bin"
  dist_root="$repo_root/dist"
  release_dir="$dist_root/$artifact"

  if [[ ! -f "$binary" ]]; then
    echo "release binary not found: $binary" >&2
    exit 1
  fi

  rm -rf "$release_dir"
  mkdir -p "$release_dir"
  cp "$binary" "$release_dir/rweb-clash"
  cp "$binary" "$dist_root/$artifact.bin"
  cp "$repo_root/README.md" "$release_dir/README.md"
  cp "$repo_root/LICENSE" "$release_dir/LICENSE"
  cp "$repo_root/packaging/linux/README.md" "$release_dir/LINUX.md"
  cp "$repo_root/packaging/linux/install.sh" "$repo_root/packaging/linux/install-systemd.sh" "$repo_root/packaging/linux/rweb-clash.service" "$repo_root/packaging/linux/rweb-clash-system.service" "$repo_root/packaging/linux/rweb-clash-ready.service" "$repo_root/packaging/linux/rweb-clash.env" "$repo_root/scripts/release-smoke.sh" "$repo_root/scripts/release-smoke.ps1" "$release_dir/"
  chmod +x "$release_dir/rweb-clash" "$dist_root/$artifact.bin" "$release_dir/install.sh" "$release_dir/install-systemd.sh" "$release_dir/release-smoke.sh"

  tar -C "$dist_root" -czf "$dist_root/$artifact.tar.gz" "$artifact"
  (cd "$dist_root" && sha256sum "$artifact.bin" > "$artifact.bin.sha256")
  (cd "$dist_root" && sha256sum "$artifact.tar.gz" > "$artifact.tar.gz.sha256")
  "$repo_root/scripts/verify-linux-archive.sh" --archive "$dist_root/$artifact.tar.gz"
}

"$repo_root/scripts/package-core.sh" --target "$target" --version "$version"
"$repo_root/scripts/package-rule-sets.sh"
"$repo_root/scripts/package-runtime-assets.sh"
pnpm --dir "$repo_root/web" build

case "$target" in
  linux-amd64|linux-x86_64)
    rust_target="x86_64-unknown-linux-musl"
    cross build -p rweb-clash-bin --features embedded-assets --release --locked --target "$rust_target"
    package_linux_archive "rweb-clash-linux-amd64" "$rust_target"
    ;;
  linux-arm64|linux-aarch64)
    rust_target="aarch64-unknown-linux-musl"
    cross build -p rweb-clash-bin --features embedded-assets --release --locked --target "$rust_target"
    package_linux_archive "rweb-clash-linux-arm64" "$rust_target"
    ;;
  windows-amd64|windows-x86_64|macos-arm64|macos-aarch64|macos-x86_64)
    "$repo_root/scripts/prepare-tauri-resources.sh" --target "$target"
    "$repo_root/scripts/verify-tauri-resources.sh" --target "$target"
    if [[ "$target" == macos-* ]]; then
      pnpm --dir "$repo_root/apps/desktop" tauri build \
        --config src-tauri/tauri.macos.conf.json
    else
      pnpm --dir "$repo_root/apps/desktop" tauri build
    fi
    ;;
  *)
    echo "unsupported target: $target" >&2
    exit 1
    ;;
esac
