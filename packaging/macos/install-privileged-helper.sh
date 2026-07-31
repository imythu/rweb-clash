#!/bin/sh
set -eu
helper=${1:?usage: install-privileged-helper.sh /path/to/helper /path/to/plist}
plist=${2:?usage: install-privileged-helper.sh /path/to/helper /path/to/plist}
[ "$(uname -s)" = Darwin ] || { echo "macOS is required" >&2; exit 1; }
[ -x "$helper" ] || { echo "helper is not executable" >&2; exit 1; }
/usr/bin/codesign --verify --deep --strict "$helper" || { echo "helper must be code signed" >&2; exit 1; }
payload="$(/usr/bin/mktemp -d /private/tmp/rweb-clash-helper.XXXXXX)"
trap 'rm -rf "$payload"' EXIT
/bin/cp "$helper" "$payload/rweb-clash-tun-helper"
/bin/cp "$plist" "$payload/com.rweb-clash.tun-helper.plist"
/bin/chmod 755 "$payload/rweb-clash-tun-helper"
script="/bin/mkdir -p '/Library/PrivilegedHelperTools' '/Library/LaunchDaemons' && /bin/cp '$payload/rweb-clash-tun-helper' '/Library/PrivilegedHelperTools/dev.rweb-clash.tun-helper' && /bin/chmod 755 '/Library/PrivilegedHelperTools/dev.rweb-clash.tun-helper' && /bin/cp '$payload/com.rweb-clash.tun-helper.plist' '/Library/LaunchDaemons/com.rweb-clash.tun-helper.plist' && /bin/chmod 644 '/Library/LaunchDaemons/com.rweb-clash.tun-helper.plist' && /bin/launchctl bootstrap system '/Library/LaunchDaemons/com.rweb-clash.tun-helper.plist'"
/usr/bin/osascript -e "do shell script \"$script\" with administrator privileges"
