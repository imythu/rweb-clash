#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$repo_root/packaging/manifests/rule-sets.toml"
target_dir="$repo_root/packaging/cache/rule-sets"
cache_root="$repo_root/packaging/cache"
source_repository="Loyalsoldier/clash-rules"
source_ref="release"
source_url_prefix="https://cdn.jsdelivr.net/gh/$source_repository@$source_ref/"
maximum_rule_bytes=16777216
mkdir -p "$cache_root"
staging_root="$(mktemp -d "$cache_root/.rule-sets.XXXXXX")"
staging_dir="$staging_root/rule-sets"
mkdir -p "$staging_dir"
trap 'rm -rf "$staging_root"' EXIT

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

if [[ "${#items[@]}" -ne 13 ]]; then
  echo "expected exactly 13 builtin rule sets, found ${#items[@]}" >&2
  exit 1
fi

commit_metadata="$staging_root/source-commit.json"
common_headers=(-H "User-Agent: rweb-clash-packager")
if [[ -n "${GITHUB_TOKEN:-}" ]]; then
  common_headers+=(-H "Authorization: Bearer $GITHUB_TOKEN")
fi
curl --fail --location --silent --show-error \
  --retry 5 --retry-delay 2 --retry-all-errors \
  --connect-timeout 20 --max-time 180 \
  "${common_headers[@]}" \
  -H "Accept: application/vnd.github+json" \
  -H "X-GitHub-Api-Version: 2022-11-28" \
  "https://api.github.com/repos/$source_repository/commits/$source_ref" \
  -o "$commit_metadata"
source_commit="$(jq -r '.sha' "$commit_metadata")"
if [[ ! "$source_commit" =~ ^[0-9a-f]{40}$ ]]; then
  echo "failed to resolve $source_repository@$source_ref to a Git commit" >&2
  exit 1
fi

manifest_out="$staging_dir/manifest.json"
printf '{\n  "schemaVersion": 1,\n  "generatedAt": "%s",\n  "source": {"repository":"%s","ref":"%s","commit":"%s"},\n  "ruleSets": [\n' \
  "$(date -u +%FT%TZ)" "$source_repository" "$source_ref" "$source_commit" > "$manifest_out"
first=1
for item in "${items[@]}"; do
  IFS='|' read -r id name url <<< "$item"
  if [[ "$url" != "$source_url_prefix"* ]]; then
    echo "unexpected rule-set source URL: $url" >&2
    exit 1
  fi
  resolved_url="${url/@$source_ref\//@$source_commit/}"
  target="$staging_dir/$id.list"
  curl -fL \
    --retry 5 \
    --retry-all-errors \
    --connect-timeout 20 \
    --max-time 180 \
    --max-filesize "$maximum_rule_bytes" \
    -H "User-Agent: rweb-clash" \
    "$resolved_url" \
    -o "$target"
  if [[ ! -s "$target" ]]; then
    echo "downloaded rule set is empty: $name ($url)" >&2
    exit 1
  fi
  rule_bytes="$(wc -c < "$target" | tr -d '[:space:]')"
  if (( rule_bytes > maximum_rule_bytes )); then
    echo "downloaded rule set exceeds $maximum_rule_bytes bytes: $name" >&2
    exit 1
  fi
  sha="$(sha256_file "$target")"
  if [[ "$first" -eq 0 ]]; then
    printf ',\n' >> "$manifest_out"
  fi
  first=0
  printf '    {"id":"%s","name":"%s","url":"%s","resolvedUrl":"%s","file":"%s.list","sha256":"%s","bytes":%s}' \
    "$id" "$name" "$url" "$resolved_url" "$id" "$sha" "$rule_bytes" >> "$manifest_out"
  echo "Downloaded $name to $target"
done
printf '\n  ]\n}\n' >> "$manifest_out"

shopt -s nullglob
rule_files=("$staging_dir"/*.list)
if [[ "${#rule_files[@]}" -ne 13 ]]; then
  echo "expected exactly 13 downloaded rule files, found ${#rule_files[@]}" >&2
  exit 1
fi

rm -rf "$target_dir"
mv "$staging_dir" "$target_dir"
echo "Wrote $target_dir/manifest.json from $source_repository@$source_commit"
