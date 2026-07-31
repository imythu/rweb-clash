# macOS privileged TUN helper

`rweb-clash-macos-helper` is installed once into `/Library/PrivilegedHelperTools`
and launched by `com.rweb-clash.tun-helper.plist`. The desktop process talks to
`/var/run/rweb-clash-tun.sock` using JSON operations (`ping`, `start`, `stop`);
it never sends arbitrary shell commands. Production installers must sign both
binaries and perform the one-time authorization during installation.
