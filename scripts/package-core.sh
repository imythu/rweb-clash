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
  echo "usage: scripts/package-core.sh --target linux-amd64|linux-arm64|windows-amd64|macos-arm64 [--version latest|vX.Y.Z]" >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
download_base="https://github.com/MetaCubeX/mihomo/releases/download"
target_dir="$repo_root/packaging/cache/cores/$target"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

case "$target" in
  windows-amd64|windows-x86_64)
    os_token="windows"; archive="zip"; binary="mihomo.exe"; arch_candidates=("amd64" "amd64-compatible")
    ;;
  macos-arm64|macos-aarch64)
    os_token="darwin"; archive="gz"; binary="mihomo"; arch_candidates=("arm64")
    ;;
  macos-x86_64)
    os_token="darwin"; archive="gz"; binary="mihomo"; arch_candidates=("amd64" "amd64-compatible")
    ;;
  linux-amd64|linux-x86_64)
    os_token="linux"; archive="gz"; binary="mihomo"; arch_candidates=("amd64" "amd64-compatible")
    ;;
  linux-arm64|linux-aarch64)
    os_token="linux"; archive="gz"; binary="mihomo"; arch_candidates=("arm64")
    ;;
  *)
    echo "unsupported target: $target" >&2
    exit 1
    ;;
esac

if [[ "$version" == "latest" ]]; then
  latest_effective_url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' https://github.com/MetaCubeX/mihomo/releases/latest)"
  version="${latest_effective_url##*/}"
fi

mkdir -p "$target_dir"
archive_path=""
asset_name=""
asset_url=""
download_errors=()
for arch in "${arch_candidates[@]}"; do
  candidate="mihomo-$os_token-$arch-$version.$archive"
  candidate_url="$download_base/$version/$candidate"
  candidate_path="$tmp_dir/$candidate"
  if curl -fL -H "User-Agent: rweb-clash" "$candidate_url" -o "$candidate_path"; then
    archive_path="$candidate_path"
    asset_name="$candidate"
    asset_url="$candidate_url"
    break
  fi
  download_errors+=("$candidate")
  rm -f "$candidate_path"
done

if [[ -z "$archive_path" ]]; then
  echo "no direct mihomo asset matched $target for $version; tried: ${download_errors[*]}" >&2
  exit 1
fi

target_binary="$target_dir/$binary"
case "$archive" in
  gz)
    gzip -dc "$archive_path" > "$target_binary"
    chmod +x "$target_binary"
    ;;
  zip)
    unzip -q "$archive_path" -d "$tmp_dir/extract"
    binary_path="$(find "$tmp_dir/extract" -type f -name 'mihomo*.exe' | head -n 1)"
    if [[ -z "$binary_path" ]]; then
      echo "downloaded archive does not contain mihomo.exe" >&2
      exit 1
    fi
    cp "$binary_path" "$target_binary"
    ;;
esac

sha="$(sha256sum "$target_binary" | awk '{print $1}')"
bytes="$(wc -c < "$target_binary" | tr -d ' ')"
cat > "$target_dir/manifest.json" <<JSON
{
  "target": "$target",
  "version": "$version",
  "asset": "$asset_name",
  "url": "$asset_url",
  "binary": "$binary",
  "sha256": "$sha",
  "bytes": $bytes,
  "generatedAt": "$(date -u +%FT%TZ)"
}
JSON
echo "Downloaded $version ($asset_name) to $target_binary"
