#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$repo_root/packaging/manifests/rule-sets.toml"
target_dir="$repo_root/packaging/cache/rule-sets"
mkdir -p "$target_dir"

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 1
fi

items=()
id=""
name=""
url=""
flush_item() {
  if [[ -n "$id" ]]; then
    items+=("$id|$name|$url")
  fi
  id=""
  name=""
  url=""
}

while IFS= read -r line; do
  line="${line#"${line%%[![:space:]]*}"}"
  line="${line%"${line##*[![:space:]]}"}"
  if [[ "$line" == "[[rule_sets]]" ]]; then
    flush_item
  elif [[ "$line" =~ ^id[[:space:]]*=[[:space:]]*\"([^\"]*)\"$ ]]; then
    id="${BASH_REMATCH[1]}"
  elif [[ "$line" =~ ^name[[:space:]]*=[[:space:]]*\"([^\"]*)\"$ ]]; then
    name="${BASH_REMATCH[1]}"
  elif [[ "$line" =~ ^url[[:space:]]*=[[:space:]]*\"([^\"]*)\"$ ]]; then
    url="${BASH_REMATCH[1]}"
  fi
done < "$manifest"
flush_item

manifest_out="$target_dir/manifest.json"
printf '{\n  "generatedAt": "%s",\n  "ruleSets": [\n' "$(date -u +%FT%TZ)" > "$manifest_out"
first=1
for item in "${items[@]}"; do
  IFS='|' read -r id name url <<< "$item"
  target="$target_dir/$id.list"
  curl -fL -H "User-Agent: rweb-clash" "$url" -o "$target"
  sha="$(sha256sum "$target" | awk '{print $1}')"
  bytes="$(wc -c < "$target" | tr -d ' ')"
  if [[ "$first" -eq 0 ]]; then
    printf ',\n' >> "$manifest_out"
  fi
  first=0
  printf '    {"id":"%s","name":"%s","url":"%s","file":"%s.list","sha256":"%s","bytes":%s}' "$id" "$name" "$url" "$id" "$sha" "$bytes" >> "$manifest_out"
  echo "Downloaded $name to $target"
done
printf '\n  ]\n}\n' >> "$manifest_out"
echo "Wrote $manifest_out"
