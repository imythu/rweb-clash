#!/usr/bin/env bash
set -euo pipefail

image=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --image|-Image)
      image="$2"
      shift 2
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$image" ]]; then
  echo "usage: scripts/verify-docker-manifest.sh --image ghcr.io/OWNER/REPO:TAG" >&2
  exit 1
fi

manifest="$(docker buildx imagetools inspect "$image")"
printf '%s\n' "$manifest"

if ! grep -qE 'Platform:[[:space:]]+linux/amd64' <<< "$manifest"; then
  echo "Docker manifest is missing linux/amd64: $image" >&2
  exit 1
fi
if ! grep -qE 'Platform:[[:space:]]+linux/arm64' <<< "$manifest"; then
  echo "Docker manifest is missing linux/arm64: $image" >&2
  exit 1
fi

echo "Docker manifest contains linux/amd64 and linux/arm64: $image"
