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
release_api="https://api.github.com/repos/MetaCubeX/mihomo/releases"
target_dir="$repo_root/packaging/cache/cores/$target"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

for command_name in curl jq; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "$command_name is required" >&2
    exit 1
  fi
done

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "sha256sum or shasum is required" >&2
    return 1
  fi
}

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

if [[ "$version" != "latest" && ! "$version" =~ ^[0-9A-Za-z._-]+$ ]]; then
  echo "invalid Mihomo release tag: $version" >&2
  exit 1
fi

common_headers=(
  -H "X-GitHub-Api-Version: 2022-11-28"
  -H "User-Agent: rweb-clash-packager"
)
if [[ -n "${GITHUB_TOKEN:-}" ]]; then
  common_headers+=(-H "Authorization: Bearer $GITHUB_TOKEN")
fi
release_metadata="$tmp_dir/release.json"
if [[ "$version" == "latest" ]]; then
  metadata_url="$release_api/latest"
else
  metadata_url="$release_api/tags/$version"
fi
curl --fail --location --silent --show-error \
  --retry 5 --retry-delay 2 --retry-all-errors \
  --connect-timeout 20 --max-time 300 \
  "${common_headers[@]}" -H "Accept: application/vnd.github+json" \
  "$metadata_url" -o "$release_metadata"
if ! jq -e '
  (.id | type == "number" and . > 0) and
  (.tag_name | type == "string" and test("^[0-9A-Za-z._-]+$")) and
  (.published_at | type == "string" and length > 0) and
  (.assets | type == "array")
' "$release_metadata" >/dev/null; then
  echo "GitHub Mihomo release metadata is invalid" >&2
  exit 1
fi
resolved_version="$(jq -r '.tag_name' "$release_metadata")"
if [[ "$version" != "latest" && "$resolved_version" != "$version" ]]; then
  echo "requested Mihomo tag $version resolved to unexpected tag $resolved_version" >&2
  exit 1
fi
version="$resolved_version"
release_id="$(jq -r '.id' "$release_metadata")"
published_at="$(jq -r '.published_at' "$release_metadata")"

mkdir -p "$target_dir"
archive_path=""
asset_name=""
asset_url=""
download_errors=()
for arch in "${arch_candidates[@]}"; do
  candidate="mihomo-$os_token-$arch-$version.$archive"
  asset_count="$(jq --arg name "$candidate" '[.assets[] | select(.name == $name)] | length' "$release_metadata")"
  if [[ "$asset_count" -ne 1 ]]; then
    download_errors+=("$candidate (metadata matches: $asset_count)")
    continue
  fi
  asset_id="$(jq -r --arg name "$candidate" '.assets[] | select(.name == $name) | .id' "$release_metadata")"
  asset_api_url="$(jq -r --arg name "$candidate" '.assets[] | select(.name == $name) | .url' "$release_metadata")"
  candidate_url="$(jq -r --arg name "$candidate" '.assets[] | select(.name == $name) | .browser_download_url' "$release_metadata")"
  expected_bytes="$(jq -r --arg name "$candidate" '.assets[] | select(.name == $name) | .size' "$release_metadata")"
  expected_sha256="$(jq -r --arg name "$candidate" '.assets[] | select(.name == $name) | .digest | sub("^sha256:"; "")' "$release_metadata")"
  asset_updated_at="$(jq -r --arg name "$candidate" '.assets[] | select(.name == $name) | .updated_at' "$release_metadata")"
  if [[ "$asset_api_url" != "https://api.github.com/repos/MetaCubeX/mihomo/releases/assets/$asset_id" \
      || "$candidate_url" != "$download_base/$version/$candidate" \
      || ! "$expected_bytes" =~ ^[0-9]+$ \
      || "$expected_bytes" -lt 1048576 \
      || "$expected_bytes" -gt 134217728 \
      || ! "$expected_sha256" =~ ^[0-9a-f]{64}$ ]]; then
    echo "invalid GitHub asset metadata for $candidate" >&2
    exit 1
  fi
  candidate_path="$tmp_dir/$candidate"
  if curl --fail --location --silent --show-error \
      --retry 5 --retry-delay 2 --retry-all-errors \
      --connect-timeout 20 --max-time 300 \
      "${common_headers[@]}" -H "Accept: application/octet-stream" \
      "$asset_api_url" -o "$candidate_path"; then
    actual_archive_bytes="$(wc -c < "$candidate_path" | tr -d '[:space:]')"
    actual_archive_sha256="$(sha256_file "$candidate_path")"
    if [[ "$actual_archive_bytes" != "$expected_bytes" || "$actual_archive_sha256" != "$expected_sha256" ]]; then
      echo "Mihomo archive verification failed for $candidate" >&2
      exit 1
    fi
    archive_path="$candidate_path"
    asset_name="$candidate"
    asset_url="$candidate_url"
    archive_bytes="$actual_archive_bytes"
    archive_sha256="$actual_archive_sha256"
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

sha="$(sha256_file "$target_binary")"
bytes="$(wc -c < "$target_binary" | tr -d ' ')"
cat > "$target_dir/manifest.json" <<JSON
{
  "target": "$target",
  "version": "$version",
  "releaseId": $release_id,
  "publishedAt": "$published_at",
  "assetId": $asset_id,
  "assetUpdatedAt": "$asset_updated_at",
  "asset": "$asset_name",
  "url": "$asset_url",
  "archiveSha256": "$archive_sha256",
  "archiveBytes": $archive_bytes,
  "binary": "$binary",
  "sha256": "$sha",
  "bytes": $bytes,
  "generatedAt": "$(date -u +%FT%TZ)"
}
JSON
echo "Downloaded $version ($asset_name) to $target_binary"
