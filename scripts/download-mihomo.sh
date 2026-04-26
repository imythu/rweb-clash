#!/usr/bin/env bash
set -euo pipefail

latest_url="https://github.com/MetaCubeX/mihomo/releases/latest"
download_base="https://github.com/MetaCubeX/mihomo/releases/download"
target_dir="$(pwd)/cache-core"
tmp_dir="$(mktemp -d)"

cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 1
fi

case "$(uname -s)" in
  Darwin)
    os_token="darwin"
    archive_ext="gz"
    binary_name="mihomo"
    ;;
  Linux)
    os_token="linux"
    archive_ext="gz"
    binary_name="mihomo"
    ;;
  MINGW*|MSYS*|CYGWIN*)
    os_token="windows"
    archive_ext="zip"
    binary_name="mihomo.exe"
    ;;
  *)
    echo "unsupported OS: $(uname -s)" >&2
    exit 1
    ;;
esac

case "$(uname -m)" in
  x86_64|amd64)
    arch_names=("amd64" "amd64-compatible")
    ;;
  i386|i686)
    arch_names=("386")
    ;;
  arm64|aarch64)
    arch_names=("arm64")
    ;;
  armv7*|armv7l)
    arch_names=("armv7" "arm")
    ;;
  arm*)
    arch_names=("arm")
    ;;
  *)
    echo "unsupported architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

mkdir -p "$target_dir"
target_binary="$target_dir/$binary_name"

latest_effective_url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$latest_url")"
tag="${latest_effective_url##*/}"
if [[ -z "$tag" || "$tag" == "latest" ]]; then
  echo "failed to resolve latest mihomo tag from $latest_url" >&2
  exit 1
fi

archive_path=""
asset_name=""
for arch_name in "${arch_names[@]}"; do
  candidate="mihomo-$os_token-$arch_name-$tag.$archive_ext"
  candidate_url="$download_base/$tag/$candidate"
  candidate_path="$tmp_dir/$candidate"
  if curl -fL -H "User-Agent: rweb-clash" "$candidate_url" -o "$candidate_path"; then
    archive_path="$candidate_path"
    asset_name="$candidate"
    break
  fi
  rm -f "$candidate_path"
done

if [[ -z "$archive_path" ]]; then
  echo "no direct mihomo asset matched this build machine" >&2
  exit 1
fi

case "$archive_ext" in
  gz)
    gzip -dc "$archive_path" > "$target_binary"
    chmod +x "$target_binary"
    ;;
  zip)
    if ! command -v unzip >/dev/null 2>&1; then
      echo "unzip is required for zip assets" >&2
      exit 1
    fi
    extract_dir="$tmp_dir/extract"
    mkdir -p "$extract_dir"
    unzip -q "$archive_path" -d "$extract_dir"
    binary_path="$(find "$extract_dir" -type f -name 'mihomo*.exe' | head -n 1)"
    if [[ -z "$binary_path" ]]; then
      echo "downloaded archive does not contain mihomo.exe" >&2
      exit 1
    fi
    cp "$binary_path" "$target_binary"
    ;;
  *)
    echo "unsupported archive extension: $archive_ext" >&2
    exit 1
    ;;
esac

echo "Downloaded $tag ($asset_name) to $target_binary"
