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

if [[ -z "$target" ]]; then
  echo "usage: scripts/prepare-tauri-resources.sh --target macos-arm64|windows-amd64" >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
resource_root="$repo_root/apps/desktop/src-tauri/resources"
core_source="$repo_root/packaging/cache/cores/$target"
rule_set_source="$repo_root/packaging/cache/rule-sets"
runtime_source="$repo_root/packaging/cache/runtime"
core_dest="$resource_root/core"
rule_set_dest="$resource_root/rule-sets"
runtime_dest="$resource_root/runtime"
macos_helper_dest="$resource_root/macos"

if [[ ! -d "$core_source" ]]; then
  echo "core cache not found: $core_source" >&2
  exit 1
fi
if [[ ! -d "$rule_set_source" || ! -s "$rule_set_source/manifest.json" ]]; then
  echo "rule-set cache or manifest not found: $rule_set_source" >&2
  exit 1
fi
if [[ ! -s "$runtime_source/geoip.metadb" || ! -s "$runtime_source/manifest.json" ]]; then
  echo "verified runtime asset cache not found: $runtime_source" >&2
  exit 1
fi

rm -rf "$resource_root"
mkdir -p "$core_dest" "$rule_set_dest" "$runtime_dest"

if [[ -f "$core_source/mihomo" ]]; then
  cp "$core_source/mihomo" "$core_dest/"
elif [[ -f "$core_source/mihomo.exe" ]]; then
  cp "$core_source/mihomo.exe" "$core_dest/"
else
  echo "no Mihomo binary found in $core_source" >&2
  exit 1
fi

shopt -s nullglob
rule_files=("$rule_set_source"/*.list)
if [[ "${#rule_files[@]}" -eq 0 ]]; then
  echo "no rule-set list files found in $rule_set_source" >&2
  exit 1
fi
cp "${rule_files[@]}" "$rule_set_dest/"
cp "$rule_set_source/manifest.json" "$rule_set_dest/"
cp "$runtime_source/geoip.metadb" "$runtime_source/manifest.json" "$runtime_dest/"

case "$target" in
  macos-arm64|macos-aarch64)
    helper_rust_target="aarch64-apple-darwin"
    ;;
  macos-x86_64)
    helper_rust_target="x86_64-apple-darwin"
    ;;
esac

if [[ -n "${helper_rust_target:-}" ]]; then
  cargo build \
    --manifest-path "$repo_root/crates/rweb-clash-macos-helper/Cargo.toml" \
    --release --locked --target "$helper_rust_target"
  mkdir -p "$macos_helper_dest"
  cp "$repo_root/target/$helper_rust_target/release/rweb-clash-macos-helper" \
    "$macos_helper_dest/rweb-clash-macos-helper"
  cp "$repo_root/packaging/macos/com.rweb-clash.tun-helper.plist" "$macos_helper_dest/"
  chmod 755 "$macos_helper_dest/rweb-clash-macos-helper"
fi

echo "Prepared Tauri resources for $target at $resource_root"
