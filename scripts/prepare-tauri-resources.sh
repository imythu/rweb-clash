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
core_dest="$resource_root/core"
rule_set_dest="$resource_root/rule-sets"

if [[ ! -d "$core_source" ]]; then
  echo "core cache not found: $core_source" >&2
  exit 1
fi
if [[ ! -d "$rule_set_source" ]]; then
  echo "rule-set cache not found: $rule_set_source" >&2
  exit 1
fi

rm -rf "$resource_root"
mkdir -p "$core_dest" "$rule_set_dest"

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
if [[ -f "$rule_set_source/manifest.json" ]]; then
  cp "$rule_set_source/manifest.json" "$rule_set_dest/"
fi

echo "Prepared Tauri resources for $target at $resource_root"
