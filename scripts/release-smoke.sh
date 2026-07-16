#!/usr/bin/env bash
set -euo pipefail

binary="target/debug/rweb-clash-bin"
listen="127.0.0.1:32990"
runner=""
verify_embedded_assets=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --binary|-Binary)
      binary="$2"
      shift 2
      ;;
    --listen|-Listen)
      listen="$2"
      shift 2
      ;;
    --runner|-Runner)
      runner="$2"
      shift 2
      ;;
    --verify-embedded-assets|-VerifyEmbeddedAssets)
      verify_embedded_assets=1
      shift
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

root="$(pwd)/.tmp-smoke-runtime"
stdout="$(pwd)/.tmp-smoke.out.log"
stderr="$(pwd)/.tmp-smoke.err.log"
pid=""

cleanup() {
  if [[ -n "$pid" ]] && kill -0 "$pid" >/dev/null 2>&1; then
    kill "$pid" >/dev/null 2>&1 || true
    wait "$pid" >/dev/null 2>&1 || true
  fi
  rm -rf "$root" "$stdout" "$stderr"
}
trap cleanup EXIT

rm -rf "$root" "$stdout" "$stderr"
mkdir -p "$root"

if [[ -n "$runner" ]]; then
  read -r -a runner_args <<< "$runner"
  "${runner_args[@]}" "$binary" --listen "$listen" --data-dir "$root" --log-level warn >"$stdout" 2>"$stderr" &
else
  "$binary" --listen "$listen" --data-dir "$root" --log-level warn >"$stdout" 2>"$stderr" &
fi
pid="$!"

for _ in $(seq 1 40); do
  if setup="$(curl -fsS "http://$listen/api/setup/status" 2>/dev/null)" \
    && diagnostics="$(curl -fsS "http://$listen/api/diagnostics/export" 2>/dev/null)" \
    && printf '%s' "$diagnostics" | grep -q '^# rweb-clash diagnostics'; then
    if [[ "$verify_embedded_assets" -eq 1 ]]; then
      core="$root/cache-core/mihomo"
      geoip="$root/data/profiles/geoip.metadb"
      rule_set_dir="$root/data/profiles/rule-sets"
      if [[ ! -x "$core" || ! -s "$geoip" ]]; then
        echo "embedded Mihomo core or GeoIP database was not restored" >&2
        exit 1
      fi
      rule_set_count="$(find "$rule_set_dir" -maxdepth 1 -type f -name '*.list' | wc -l | tr -d ' ')"
      if [[ "$rule_set_count" -ne 13 ]]; then
        echo "expected 13 embedded rule sets, found $rule_set_count" >&2
        exit 1
      fi
      core_status="$(curl -fsS -X POST "http://$listen/api/core/start")"
      if ! printf '%s' "$core_status" | grep -q '"state":"running"'; then
        echo "Mihomo did not reach the running state: $core_status" >&2
        exit 1
      fi
      curl -fsS -X POST "http://$listen/api/core/stop" >/dev/null
    fi
    printf '{"ok":true,"setup":%s}\n' "$setup"
    exit 0
  fi
  sleep 0.5
done

echo "rweb-clash did not become ready on $listen" >&2
echo "--- stdout ---" >&2
cat "$stdout" >&2 || true
echo "--- stderr ---" >&2
cat "$stderr" >&2 || true
exit 1
