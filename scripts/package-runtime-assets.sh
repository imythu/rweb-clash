#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_manifest="$repo_root/packaging/manifests/runtime-assets.toml"
target_dir="$repo_root/packaging/cache/runtime"

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

read_string() {
  local key="$1"
  sed -nE "s/^[[:space:]]*${key}[[:space:]]*=[[:space:]]*\"([^\"]*)\"[[:space:]]*$/\\1/p" "$source_manifest"
}

read_integer() {
  local key="$1"
  sed -nE "s/^[[:space:]]*${key}[[:space:]]*=[[:space:]]*([0-9]+)[[:space:]]*$/\\1/p" "$source_manifest"
}

id="$(read_string id)"
file="$(read_string file)"
logical_path="$(read_string logical_path)"
repository="$(read_string repository)"
latest_release_api="$(read_string latest_release_api)"
asset_name="$(read_string asset_name)"
minimum_bytes="$(read_integer minimum_bytes)"
maximum_bytes="$(read_integer maximum_bytes)"

required_values=(
  "$id" "$file" "$logical_path" "$repository" "$latest_release_api"
  "$asset_name" "$minimum_bytes" "$maximum_bytes"
)
for value in "${required_values[@]}"; do
  if [[ -z "$value" ]]; then
    echo "runtime asset manifest is missing a required value: $source_manifest" >&2
    exit 1
  fi
done

if [[ "$file" == */* || "$file" == *\\* || "$asset_name" != "$file" || "$logical_path" != "runtime/$file" ]]; then
  echo "invalid runtime asset file or logical path in $source_manifest" >&2
  exit 1
fi
expected_latest_release_api="https://api.github.com/repos/$repository/releases/latest"
if [[ "$latest_release_api" != "$expected_latest_release_api" ]]; then
  echo "runtime asset latest release API does not match repository $repository" >&2
  exit 1
fi
if (( minimum_bytes <= 0 || minimum_bytes > maximum_bytes )); then
  echo "invalid runtime asset size bounds in $source_manifest" >&2
  exit 1
fi

mkdir -p "$target_dir"
temp_release="$(mktemp "$target_dir/.release.json.XXXXXX")"
temp_asset="$(mktemp "$target_dir/.${file}.download.XXXXXX")"
temp_manifest="$(mktemp "$target_dir/.manifest.json.XXXXXX")"
cleanup() {
  rm -f "$temp_release" "$temp_asset" "$temp_manifest"
}
trap cleanup EXIT

common_headers=(
  -H "X-GitHub-Api-Version: 2022-11-28"
  -H "User-Agent: rweb-clash-packager"
)
if [[ -n "${GITHUB_TOKEN:-}" ]]; then
  common_headers+=(-H "Authorization: Bearer $GITHUB_TOKEN")
fi

curl --fail --location --silent --show-error \
  --retry 5 --retry-delay 2 --retry-all-errors \
  --connect-timeout 20 --max-time 300 \
  "${common_headers[@]}" -H "Accept: application/vnd.github+json" \
  "$latest_release_api" -o "$temp_release"

if ! jq -e --arg name "$asset_name" '
  (.id | type == "number") and
  (.tag_name | type == "string" and length > 0) and
  (.published_at | type == "string" and length > 0) and
  ([.assets[] | select(.name == $name)] | length == 1) and
  ([.assets[] | select(.name == $name)][0] |
    (.id | type == "number") and
    (.size | type == "number") and
    (.url | type == "string" and length > 0) and
    (.browser_download_url | type == "string" and length > 0) and
    (.updated_at | type == "string" and length > 0) and
    (.digest | type == "string" and test("^sha256:[0-9a-f]{64}$")))
' "$temp_release" >/dev/null; then
  echo "GitHub latest release metadata did not contain one verifiable $asset_name asset" >&2
  exit 1
fi

release_id="$(jq -r '.id' "$temp_release")"
release_tag="$(jq -r '.tag_name' "$temp_release")"
published_at="$(jq -r '.published_at' "$temp_release")"
asset_id="$(jq -r --arg name "$asset_name" '.assets[] | select(.name == $name) | .id' "$temp_release")"
api_url="$(jq -r --arg name "$asset_name" '.assets[] | select(.name == $name) | .url' "$temp_release")"
browser_url="$(jq -r --arg name "$asset_name" '.assets[] | select(.name == $name) | .browser_download_url' "$temp_release")"
asset_updated_at="$(jq -r --arg name "$asset_name" '.assets[] | select(.name == $name) | .updated_at' "$temp_release")"
expected_bytes="$(jq -r --arg name "$asset_name" '.assets[] | select(.name == $name) | .size' "$temp_release")"
expected_sha256="$(jq -r --arg name "$asset_name" '.assets[] | select(.name == $name) | .digest | sub("^sha256:"; "")' "$temp_release")"

expected_asset_api_url="https://api.github.com/repos/$repository/releases/assets/$asset_id"
browser_url_prefix="https://github.com/$repository/releases/download/"
if [[ "$api_url" != "$expected_asset_api_url" || "$browser_url" != "$browser_url_prefix"*"/$asset_name" ]]; then
  echo "GitHub runtime asset URLs do not match repository $repository and asset $asset_id" >&2
  exit 1
fi
if (( expected_bytes < minimum_bytes || expected_bytes > maximum_bytes )); then
  echo "GitHub runtime asset size is outside the allowed range: $expected_bytes bytes" >&2
  exit 1
fi

curl --fail --location --silent --show-error \
  --retry 5 --retry-delay 2 --retry-all-errors \
  --connect-timeout 20 --max-time 300 \
  "${common_headers[@]}" -H "Accept: application/octet-stream" \
  "$api_url" -o "$temp_asset"

actual_bytes="$(wc -c < "$temp_asset" | tr -d '[:space:]')"
if (( actual_bytes < minimum_bytes || actual_bytes > maximum_bytes )); then
  echo "runtime asset size is outside the allowed range: $actual_bytes bytes" >&2
  exit 1
fi
if [[ "$actual_bytes" != "$expected_bytes" ]]; then
  echo "runtime asset size mismatch: expected $expected_bytes, got $actual_bytes" >&2
  exit 1
fi

actual_sha256="$(sha256_file "$temp_asset")"
if [[ "$actual_sha256" != "$expected_sha256" ]]; then
  echo "runtime asset SHA256 mismatch: expected $expected_sha256, got $actual_sha256" >&2
  exit 1
fi

jq -n \
  --arg generatedAt "$(date -u +%FT%TZ)" \
  --arg id "$id" \
  --arg file "$file" \
  --arg logicalPath "$logical_path" \
  --arg repository "$repository" \
  --argjson releaseId "$release_id" \
  --arg releaseTag "$release_tag" \
  --argjson assetId "$asset_id" \
  --arg apiUrl "$api_url" \
  --arg browserUrl "$browser_url" \
  --arg publishedAt "$published_at" \
  --arg assetUpdatedAt "$asset_updated_at" \
  --argjson bytes "$actual_bytes" \
  --arg sha256 "$actual_sha256" \
  '{
    schemaVersion: 1,
    generatedAt: $generatedAt,
    assets: [{
      id: $id,
      file: $file,
      logicalPath: $logicalPath,
      repository: $repository,
      releaseId: $releaseId,
      releaseTag: $releaseTag,
      assetId: $assetId,
      apiUrl: $apiUrl,
      browserUrl: $browserUrl,
      publishedAt: $publishedAt,
      assetUpdatedAt: $assetUpdatedAt,
      bytes: $bytes,
      sha256: $sha256
    }]
  }' > "$temp_manifest"

mv -f "$temp_asset" "$target_dir/$file"
mv -f "$temp_manifest" "$target_dir/manifest.json"
echo "Downloaded runtime asset $logical_path from release $release_tag (asset $asset_id)"
