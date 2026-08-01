#!/usr/bin/env bash
set -euo pipefail

target=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --target|-Target)
      target="$2"
      shift 2
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

case "$target" in
  macos-arm64|macos-aarch64|macos-x86_64)
    core_name="mihomo"
    unexpected_core="mihomo.exe"
    ;;
  windows-amd64|windows-x86_64)
    core_name="mihomo.exe"
    unexpected_core="mihomo"
    ;;
  *)
    echo "usage: scripts/verify-tauri-resources.sh --target macos-arm64|macos-aarch64|macos-x86_64|windows-amd64|windows-x86_64" >&2
    exit 1
    ;;
esac

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required" >&2
  exit 1
fi

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

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
resource_root="$repo_root/apps/desktop/src-tauri/resources"
rule_source_manifest="$repo_root/packaging/manifests/rule-sets.toml"
core_dir="$resource_root/core"
rule_set_dir="$resource_root/rule-sets"
runtime_dir="$resource_root/runtime"

case "$target" in
  windows-amd64|windows-x86_64)
    windows_helper="$resource_root/windows/rweb-clash-windows-helper.exe"
    if [[ ! -s "$windows_helper" ]]; then
      echo "Windows privileged TUN helper resource is missing or empty: $windows_helper" >&2
      exit 1
    fi
    ;;
esac

if [[ ! -d "$core_dir" ]]; then
  echo "Tauri core resource directory not found: $core_dir" >&2
  exit 1
fi
if [[ ! -d "$rule_set_dir" ]]; then
  echo "Tauri rule-set resource directory not found: $rule_set_dir" >&2
  exit 1
fi
if [[ ! -d "$runtime_dir" ]]; then
  echo "Tauri runtime resource directory not found: $runtime_dir" >&2
  exit 1
fi
if [[ ! -s "$core_dir/$core_name" ]]; then
  echo "Tauri Mihomo resource not found or empty for $target: $core_dir/$core_name" >&2
  exit 1
fi
if [[ -f "$core_dir/$unexpected_core" ]]; then
  echo "Unexpected Mihomo resource for $target: $core_dir/$unexpected_core" >&2
  exit 1
fi

case "$target" in
  macos-arm64|macos-aarch64|macos-x86_64)
    macos_helper_dir="$resource_root/macos"
    macos_helper="$macos_helper_dir/rweb-clash-macos-helper"
    macos_helper_plist="$macos_helper_dir/com.rweb-clash.tun-helper.plist"
    if [[ ! -x "$macos_helper" || ! -s "$macos_helper_plist" ]]; then
      echo "macOS privileged TUN helper resource is missing or not executable: $macos_helper" >&2
      exit 1
    fi
    ;;
esac

shopt -s nullglob
rule_files=("$rule_set_dir"/*.list)
rule_manifest="$rule_set_dir/manifest.json"
if [[ ! -s "$rule_manifest" ]]; then
  echo "Tauri rule-set manifest is missing or empty: $rule_manifest" >&2
  exit 1
fi

expected_rule_ids=()
while IFS= read -r rule_id; do
  expected_rule_ids+=("$rule_id")
done < <(sed -nE 's/^[[:space:]]*id[[:space:]]*=[[:space:]]*"([^"]+)"[[:space:]]*$/\1/p' "$rule_source_manifest" | sort)
if [[ "${#expected_rule_ids[@]}" -ne 13 || "$(printf '%s\n' "${expected_rule_ids[@]}" | uniq | wc -l | tr -d '[:space:]')" -ne 13 ]]; then
  echo "Rule-set source manifest must define exactly 13 unique IDs: $rule_source_manifest" >&2
  exit 1
fi

if ! jq -e '
  .schemaVersion == 1 and
  (.source.repository == "Loyalsoldier/clash-rules") and
  (.source.ref == "release") and
  (.source.commit | type == "string" and test("^[0-9a-f]{40}$")) and
  (.source.commit as $commit |
  (.ruleSets | type == "array" and length == 13) and
  (all(.ruleSets[];
    (.id | type == "string" and length > 0) and
    (.name | type == "string" and length > 0) and
    (.url | type == "string" and test("^https://cdn\\.jsdelivr\\.net/gh/Loyalsoldier/clash-rules@release/[^/]+\\.txt$")) and
    (.resolvedUrl == (.url | sub("@release/"; "@" + $commit + "/"))) and
    (.file | type == "string") and
    (.file == (.id + ".list")) and
    (.bytes | type == "number" and . > 0 and . <= 16777216) and
    (.sha256 | type == "string" and test("^[0-9a-f]{64}$")))) and
  ([.ruleSets[].id] | unique | length == 13) and
  ([.ruleSets[].file] | unique | length == 13))
' "$rule_manifest" >/dev/null; then
  echo "Tauri rule-set manifest is invalid: $rule_manifest" >&2
  exit 1
fi

expected_ids_text="$(printf '%s\n' "${expected_rule_ids[@]}")"
manifest_ids_text="$(jq -r '.ruleSets[].id' "$rule_manifest" | sort)"
if [[ "$manifest_ids_text" != "$expected_ids_text" ]]; then
  echo "Tauri rule-set manifest IDs do not match $rule_source_manifest" >&2
  exit 1
fi

expected_files_text="$(printf '%s.list\n' "${expected_rule_ids[@]}" | sort)"
manifest_files_text="$(jq -r '.ruleSets[].file' "$rule_manifest" | sort)"
actual_files_text="$(for rule_file in "${rule_files[@]}"; do basename "$rule_file"; done | sort)"
if [[ "${#rule_files[@]}" -ne 13 || "$manifest_files_text" != "$expected_files_text" || "$actual_files_text" != "$expected_files_text" ]]; then
  echo "Tauri rule-set files must exactly match the 13 entries in $rule_source_manifest" >&2
  exit 1
fi

for rule_file in "${rule_files[@]}"; do
  file_name="$(basename "$rule_file")"
  expected_rule_bytes="$(jq -r --arg file "$file_name" '.ruleSets[] | select(.file == $file) | .bytes' "$rule_manifest")"
  actual_rule_bytes="$(wc -c < "$rule_file" | tr -d '[:space:]')"
  if [[ "$actual_rule_bytes" != "$expected_rule_bytes" ]]; then
    echo "Tauri rule-set resource size mismatch for $file_name: expected $expected_rule_bytes, got $actual_rule_bytes" >&2
    exit 1
  fi
  expected_rule_sha256="$(jq -r --arg file "$file_name" '.ruleSets[] | select(.file == $file) | .sha256' "$rule_manifest")"
  actual_rule_sha256="$(sha256_file "$rule_file")"
  if [[ "$actual_rule_sha256" != "$expected_rule_sha256" ]]; then
    echo "Tauri rule-set resource SHA256 mismatch for $file_name: expected $expected_rule_sha256, got $actual_rule_sha256" >&2
    exit 1
  fi
done

runtime_asset="$runtime_dir/geoip.metadb"
runtime_manifest="$runtime_dir/manifest.json"
if [[ ! -s "$runtime_asset" || ! -s "$runtime_manifest" ]]; then
  echo "Tauri GeoIP runtime resource or manifest is missing or empty: $runtime_dir" >&2
  exit 1
fi
if ! jq -e '
  .schemaVersion == 1 and
  (.assets | type == "array" and length == 1) and
  (.assets[0] |
    .id == "geoip" and
    .file == "geoip.metadb" and
    .logicalPath == "runtime/geoip.metadb" and
    .repository == "MetaCubeX/meta-rules-dat" and
    (.releaseId | type == "number" and . > 0) and
    (.releaseTag | type == "string" and length > 0) and
    (.assetId | type == "number" and . > 0) and
    (.apiUrl | type == "string" and test("^https://api\\.github\\.com/repos/MetaCubeX/meta-rules-dat/releases/assets/[0-9]+$")) and
    (.browserUrl | type == "string" and test("^https://github\\.com/MetaCubeX/meta-rules-dat/releases/download/.+/geoip\\.metadb$")) and
    (.publishedAt | type == "string" and length > 0) and
    (.assetUpdatedAt | type == "string" and length > 0) and
    (.bytes | type == "number" and . >= 1048576 and . <= 67108864) and
    (.sha256 | type == "string" and test("^[0-9a-f]{64}$")))
' "$runtime_manifest" >/dev/null; then
  echo "Tauri runtime asset manifest is invalid: $runtime_manifest" >&2
  exit 1
fi
expected_bytes="$(jq -r '.assets[0].bytes' "$runtime_manifest")"
actual_bytes="$(wc -c < "$runtime_asset" | tr -d '[:space:]')"
if [[ "$actual_bytes" != "$expected_bytes" ]]; then
  echo "Tauri GeoIP runtime resource size mismatch: expected $expected_bytes, got $actual_bytes" >&2
  exit 1
fi
expected_sha256="$(jq -r '.assets[0].sha256' "$runtime_manifest")"
actual_sha256="$(sha256_file "$runtime_asset")"
if [[ "$actual_sha256" != "$expected_sha256" ]]; then
  echo "Tauri GeoIP runtime resource SHA256 mismatch: expected $expected_sha256, got $actual_sha256" >&2
  exit 1
fi

echo "Verified Tauri resources for $target at $resource_root"
