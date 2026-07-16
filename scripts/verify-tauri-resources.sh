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
  macos-arm64)
    core_name="mihomo"
    unexpected_core="mihomo.exe"
    ;;
  windows-amd64)
    core_name="mihomo.exe"
    unexpected_core="mihomo"
    ;;
  *)
    echo "usage: scripts/verify-tauri-resources.sh --target macos-arm64|windows-amd64" >&2
    exit 1
    ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
resource_root="$repo_root/apps/desktop/src-tauri/resources"
core_dir="$resource_root/core"
rule_set_dir="$resource_root/rule-sets"

if [[ ! -d "$core_dir" ]]; then
  echo "Tauri core resource directory not found: $core_dir" >&2
  exit 1
fi
if [[ ! -d "$rule_set_dir" ]]; then
  echo "Tauri rule-set resource directory not found: $rule_set_dir" >&2
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

shopt -s nullglob
rule_files=("$rule_set_dir"/*.list)
if [[ "${#rule_files[@]}" -eq 0 ]]; then
  echo "No Tauri rule-set list files found in $rule_set_dir" >&2
  exit 1
fi
for file in "${rule_files[@]}"; do
  if [[ ! -s "$file" ]]; then
    echo "Tauri rule-set resource is empty: $file" >&2
    exit 1
  fi
done

echo "Verified Tauri resources for $target at $resource_root"
