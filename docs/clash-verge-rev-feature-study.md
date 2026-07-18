# clash-verge-rev Feature Study

Reference checkout:

- Repository: `https://github.com/clash-verge-rev/clash-verge-rev`
- Checkout: temporary shallow clone outside this repository
- Branch: `dev`
- Commit: `b31437f7277f048a30e26b44b76f9282c095030f`
- Checkout: clean, shallow clone

The reference project is GPL-3.0-only while rweb-clash is MIT. The items below
describe behavior and product ideas only. Implementations, UI text, styles,
icons, update keys, and source code must be independently created, and every
new dependency needs its own license review.

## Recommended Features

| Priority | Feature | Value | Cost | Suggested rweb-clash approach | Reference evidence |
| --- | --- | --- | --- | --- | --- |
| 1 | Real-time connection workspace | High | Medium-high | Stream connections through the shared Rust backend with SSE or WebSocket; add history, search, sorting, details, and close-one/close-all actions. | `src/pages/connections.tsx`, `src/hooks/use-connection-data.ts`, `src/components/connection/connection-detail.tsx` |
| 2 | Backup, restore, and retention | High | Medium-high | Back up SQLite and required source assets with a schema manifest. Exclude generated `runtime.yaml`. Add local archives first and WebDAV later with encrypted credentials. | `src-tauri/src/core/backup.rs`, `src-tauri/src/module/auto_backup.rs`, `src-tauri/src/cmd/webdav.rs` |
| 3 | Signed application updates | High | High | Update the desktop bundle and its compatible Mihomo binary together. Require owned signing keys, CI manifests, verification, and rollback. | `src-tauri/src/core/updater.rs`, `src-tauri/tauri.conf.json`, `src/services/update.ts` |
| 4 | Per-source download route | High | Medium | Let each subscription or rule set choose direct, running core, system proxy, or guarded automatic fallback. Preserve the current SSRF, DNS, redirect, and size checks and report the route actually used. | `src-tauri/src/utils/network.rs`, `src-tauri/src/config/prfitem.rs`, `src-tauri/src/feat/profile.rs` |
| 5 | OS login launch and silent start | High | Low-medium | Add a desktop-only preference backed by the official Tauri autostart plugin. Keep it separate from the existing core `auto_start` setting. | `src-tauri/src/core/autostart.rs`, `src/components/setting/setting-system.tsx` |
| 6 | Rich tray controls and speed | High | Medium | Add system proxy, TUN, mode, restart, close-all-connections, and live rates. Limit group entries so native menus remain usable. | `src-tauri/src/core/tray/mod.rs`, `src-tauri/src/core/tray/speed_task.rs` |
| 7 | System notifications | High | Low-medium | Notify on core crashes, repeated subscription failures, imminent expiry, and proxy recovery failures, with per-event switches and deduplication. | `src-tauri/src/utils/notification.rs`, `src-tauri/src/core/notification.rs` |
| 8 | Clipboard, file, and share-link import | Medium-high | Medium | Add a bounded raw-content import API and store parsed assets as local sources while preserving SQLite as the source of truth. | `src/utils/uri-parser/index.ts`, `src/pages/profiles.tsx`, `src/components/profile/file-input.tsx` |
| 9 | Configurable latency testing | Medium-high | Low | Expose a validated URL, timeout, and sorting settings; reuse the existing group URL and delay fields. | `src-tauri/src/config/verge.rs`, `src/components/setting/mods/misc-viewer.tsx` |
| 10 | Proxy bypass, PAC, and ownership-aware guard | Medium-high | High | Extend the existing platform backup/restore ownership model. A guard must never overwrite changes made by the user or another application. | `src/components/setting/mods/sysproxy-viewer.tsx`, `src-tauri/src/core/sysopt.rs` |
| 11 | Typed advanced DNS settings | Medium | Medium-high | Model nameservers, fallback, fake-IP filters, policy, and hosts in SQLite with URL/domain/CIDR validation. Do not expose raw generated YAML editing. | `src/components/setting/mods/dns-viewer.tsx`, `src-tauri/src/config/clash.rs` |
| 12 | Internationalization and system theme | Medium | Medium-high | Start with Chinese/English and light/dark/system. Write original translations and preserve dense operational layouts. | `src/services/i18n.ts`, `src/components/setting/setting-verge-basic.tsx` |
| 13 | Global shortcuts | Medium | Medium | Use the official desktop plugin, detect registration conflicts, prevent duplicates, and unregister on shutdown. | `src-tauri/src/core/hotkey.rs`, `src/components/setting/mods/hotkey-viewer.tsx` |

## Current Gaps That Raise Priority

- Connections are a reduced polling response rather than a dedicated live
  workspace.
- Diagnostics can be exported, but application state cannot be backed up or
  restored.
- The desktop tray only shows the window, starts/stops the core, and exits.
- `auto_start` currently means starting Mihomo after the backend starts; it
  does not register the desktop application at OS login.
- The delay URL and timeout are fixed even though the storage model already
  contains related group fields.
- Remote subscriptions and rule sets currently use a direct client only; hosts
  that require the running core for outbound access need an explicit, safe
  route policy rather than an implicit system-proxy fallback.
- DNS exposes enable/mode while nameservers are generated from fixed values.

## Features Not Recommended for Direct Adoption

Raw Profile switching, YAML editing, Merge/Script injection, and CSS injection
conflict with rweb-clash's SQLite source-of-truth model and generated runtime
configuration. Adding them would create two competing configuration owners and
weaken the validation and rollback guarantees in the backend.

The upstream initialization path also calls `start_core()` unconditionally
from its manager bootstrap. That behavior directly conflicts with rweb-clash's
explicit `auto_start` intent and must not be adopted; only its serialized,
idempotent lifecycle and failure rollback patterns are useful references.

The upstream egress card rotates across several providers, but its query cache
is memory-only and the result does not identify its source domain or explicitly
choose direct versus core-proxied transport. rweb-clash's persistent cached
result and Rust-side route selection should remain authoritative. A future
multi-provider fallback should use stable priority and health backoff rather
than random ordering.
