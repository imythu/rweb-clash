# macOS privileged TUN helper

The signed macOS app bundles `rweb-clash-macos-helper` and its launchd plist. The
first TUN start installs the helper into `/Library/PrivilegedHelperTools` and
launches it through `/Library/LaunchDaemons`; that is the only expected password
prompt. The desktop process then talks to `/var/run/rweb-clash-tun.sock` using
JSON operations (`ping`, `start`, `stop`), and never sends arbitrary shell
commands. `install-privileged-helper.sh` remains available for manual package
integration.
