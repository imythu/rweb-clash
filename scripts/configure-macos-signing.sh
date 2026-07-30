#!/usr/bin/env bash
set -euo pipefail

if [[ -z "${APPLE_CERTIFICATE:-}" || -z "${APPLE_CERTIFICATE_PASSWORD:-}" ]]; then
  echo "No Apple signing certificate configured; building unsigned macOS packages."
  exit 0
fi

if [[ -z "${RUNNER_TEMP:-}" ]]; then
  echo "RUNNER_TEMP is required for macOS signing setup." >&2
  exit 1
fi

certificate_path="$RUNNER_TEMP/rweb-clash-apple-code-signing.p12"
keychain_path="$RUNNER_TEMP/rweb-clash-build.keychain-db"
keychain_password="${KEYCHAIN_PASSWORD:-$(uuidgen)}"

if ! printf '%s' "$APPLE_CERTIFICATE" | base64 -d > "$certificate_path" 2>/dev/null; then
  printf '%s' "$APPLE_CERTIFICATE" | base64 -D > "$certificate_path"
fi

security create-keychain -p "$keychain_password" "$keychain_path"
security default-keychain -s "$keychain_path"
security unlock-keychain -p "$keychain_password" "$keychain_path"
security set-keychain-settings -t 3600 -u "$keychain_path"
security import "$certificate_path" -k "$keychain_path" -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign -T /usr/bin/productsign
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$keychain_path"

security find-identity -v -p codesigning "$keychain_path"

identity="${APPLE_SIGNING_IDENTITY:-}"
if [[ -z "$identity" ]]; then
  identity="$(
    security find-identity -v -p codesigning "$keychain_path" |
      awk -F'"' '/Developer ID Application/ { print $2; exit }'
  )"
fi
if [[ -z "$identity" ]]; then
  identity="$(
    security find-identity -v -p codesigning "$keychain_path" |
      awk -F'"' '/Apple Distribution/ { print $2; exit }'
  )"
fi
if [[ -z "$identity" ]]; then
  identity="$(
    security find-identity -v -p codesigning "$keychain_path" |
      awk -F'"' '/"/ { print $2; exit }'
  )"
fi
if [[ -z "$identity" ]]; then
  echo "No Apple code signing identity found after importing certificate." >&2
  exit 1
fi

echo "APPLE_SIGNING_IDENTITY=$identity" >> "$GITHUB_ENV"
echo "Configured Apple signing identity: $identity"

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
core_path="$repo_root/apps/desktop/src-tauri/resources/core/mihomo"
if [[ ! -x "$core_path" ]]; then
  echo "Packaged macOS Mihomo core is missing or not executable: $core_path" >&2
  exit 1
fi
/usr/bin/codesign --force --timestamp --options runtime --sign "$identity" "$core_path"
/usr/bin/codesign --verify --strict --verbose=2 "$core_path"
echo "Signed packaged macOS Mihomo core: $core_path"
