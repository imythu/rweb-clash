# Release Checklist

Use this checklist for every automatic snapshot and semantic stable release run.

- A push to `master` creates a Beijing-time snapshot tag named `snapshot-YYYYMMDD-HHmm` and a prerelease.
- A pushed `vMAJOR.MINOR.PATCH-beta.N` tag creates a beta GitHub prerelease without moving the stable `latest` channel.
- A pushed `vMAJOR.MINOR.PATCH` tag creates that stable release and marks it as latest.
- A manual `workflow_dispatch` from `master` requires the same numeric `release_version` and can optionally enable desktop signing.
- Stable release tags are synchronized into Cargo and desktop package versions; snapshots use sortable `YYYY.MDD.HHmm` package versions.
- macOS uses `YYYYMMDDHHmm` for `CFBundleVersion`, and MSI uses `YY.M.D.HHmm` within WiX limits for both channels.
- Existing release tags are never overwritten, and non-numeric stable SemVer tags are rejected.

## Workflow Semantics

- Pushes to `master` publish all target artifacts, the timestamped Docker tag, the moving `snapshot` Docker tag, and a GitHub prerelease.
- Beta tag pushes publish all target artifacts and a GitHub prerelease without updating the moving `latest` Docker tag.
- Stable semantic tag pushes and manual runs publish all target artifacts, the SemVer Docker tag, the moving `latest` Docker tag, and a stable GitHub Release.
- Automatic snapshots and tag-triggered stable releases are unsigned. Manual stable releases are unsigned unless `sign_desktop` is explicitly enabled and all platform secrets are configured.
- Rule URLs intentionally track their mutable `@release` branches, so each build fetches and records the latest available snapshots.
- A shared asset job resolves one rule commit and one GeoIP release, then every platform and Docker build consumes that same verified artifact.
- `mihomo_version=latest` is resolved once to a concrete release tag; each core archive must match GitHub's asset size and SHA256 digest before extraction.

## Required GitHub Actions Jobs

- `Linux amd64 binary`
  - Checks user and system Linux install script syntax.
  - Builds static musl `rweb-clash-bin` with `embedded-assets` and `--locked`.
  - Runs `scripts/release-smoke.sh` against the release binary.
  - Verifies the tar archive contents, executable bits, license, and all systemd service templates.
  - Verifies the non-root system service grants only the `CAP_NET_ADMIN` capability required by TUN mode.
  - Exercises both user and system installation layouts.
  - Uploads `rweb-clash-linux-amd64.bin` and its checksum.
  - Uploads `rweb-clash-linux-amd64.tar.gz`.
  - Uploads `rweb-clash-linux-amd64.tar.gz.sha256`.
- `Linux arm64 binary`
  - Checks user and system Linux install script syntax.
  - Builds static musl `rweb-clash-bin` with `embedded-assets` and `--locked`.
  - Runs `scripts/release-smoke.sh` under `qemu-aarch64`.
  - Verifies the tar archive contents, executable bits, license, and all systemd service templates.
  - Verifies the non-root system service grants only the `CAP_NET_ADMIN` capability required by TUN mode.
  - Uploads `rweb-clash-linux-arm64.bin` and its checksum.
  - Uploads `rweb-clash-linux-arm64.tar.gz`.
  - Uploads `rweb-clash-linux-arm64.tar.gz.sha256`.
- `Tauri macos-arm64`
  - Verifies Tauri resources contain the macOS Mihomo core, verified GeoIP, and exactly 13 verified rule sets.
  - Builds the desktop app for `aarch64-apple-darwin`.
  - Uploads at least one `.dmg`.
  - Uploads `rweb-clash-macos-arm64.sha256`.
  - Generates checksums with macOS-compatible `shasum -a 256`.
  - Uses Apple signing and notarization secrets only when an opted-in manual release requests signing.
- `Tauri windows-amd64`
  - Verifies Tauri resources contain the Windows Mihomo core, the privileged TUN helper, verified GeoIP, and exactly 13 verified rule sets.
  - Builds the desktop app for `x86_64-pc-windows-msvc`.
  - Uploads both an `.msi` and an NSIS `.exe`.
  - Uploads `rweb-clash-windows-amd64.sha256`.
  - Uses Windows code signing secrets only when an opted-in manual release requests signing.
- `Linux Docker image`
  - Builds and loads a `linux/amd64` smoke image.
  - Runs the smoke container and checks `/api/setup/status`.
  - Checks `/api/diagnostics/export` from inside the smoke run path.
  - Runs without network access, verifies the bundled core, GeoIP, and 13 rule files, and starts/stops Mihomo.
  - Builds and pushes `linux/amd64` and `linux/arm64`.
  - Inspects the pushed Docker manifest and requires both `linux/amd64` and `linux/arm64`.
  - Publishes to lowercase `ghcr.io/<owner>/<repo>`.

## Artifact Expectations

Linux does not use Tauri. The Linux release archive must contain:

- `rweb-clash`
- `install.sh`
- `install-systemd.sh`
- `rweb-clash.service`
- `rweb-clash-system.service`
- `rweb-clash-ready.service`
- `rweb-clash.env`
- `README.md`
- `LINUX.md`
- `LICENSE`
- `release-smoke.sh`
- `release-smoke.ps1`

Desktop releases must contain platform resources:

- Mihomo core under Tauri resources.
- Verified `geoip.metadb` under Tauri resources.
- Exactly 13 verified default rule sets under Tauri resources.
- Web UI built with `pnpm --dir web build:tauri`.
- Unsigned packages must be labeled as unsigned in the Release notes.

## Signing Configuration

macOS signing and notarization use the Tauri environment variables configured in the release workflow:

- `APPLE_CERTIFICATE` is a base64-encoded `.p12` certificate.
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_SIGNING_IDENTITY`
- `APPLE_ID`
- `APPLE_PASSWORD`
- `APPLE_TEAM_ID`
- `KEYCHAIN_PASSWORD` is optional. The workflow generates a temporary password when omitted.

Windows signing is optional. Enable `sign_desktop` for releases intended for broad non-technical distribution; unsigned stable packages remain clearly labeled:

- `WINDOWS_CERTIFICATE` is a base64-encoded `.pfx` certificate.
- `WINDOWS_CERTIFICATE_PASSWORD`
- `WINDOWS_CERTIFICATE_THUMBPRINT`
- `WINDOWS_CERTIFICATE_TIMESTAMP_URL`
- `WINDOWS_CERTIFICATE_DIGEST_ALGORITHM` defaults to `sha256` when omitted.

## Manual Verification After CI

Before pushing to `master`, pushing a stable SemVer tag, or manually dispatching a stable release, run the local verification script on a developer machine:

```text
powershell -ExecutionPolicy Bypass -File scripts/verify-release-local.ps1
```

The script also checks that `.github/workflows/release.yml` still contains the required platform matrix, Linux non-Tauri binary path, Docker multiarch publishing, and artifact verification gates.
It also checks that `Dockerfile` still builds the embedded Linux binary, packages the correct Mihomo core per architecture, and exposes the expected runtime healthcheck and entrypoint.
GitHub Actions runs the same workflow structure check before building release artifacts.

1. Download `rweb-clash-linux-amd64.tar.gz` from the workflow artifacts.
2. Extract it on a Linux amd64 machine.
3. Run:

   ```text
   ./rweb-clash --listen 127.0.0.1:32990 --data-dir ./.tmp-rweb-clash
   ```

4. Open:

   ```text
   http://127.0.0.1:32990
   ```

5. Verify:

   ```text
   curl -fsS http://127.0.0.1:32990/api/setup/status
   curl -fsS http://127.0.0.1:32990/api/diagnostics/export
   ```

6. Pull and inspect Docker manifests:

   ```text
   docker buildx imagetools inspect ghcr.io/<owner>/<repo>:<tag>
   ```

   The manifest must include `linux/amd64` and `linux/arm64`.

## Blocking Conditions

Do not publish or mark the release complete if any of these are true:

- Any target job is skipped or failed.
- Any expected artifact is missing.
- Any downloaded artifact does not match its `.sha256` checksum.
- Docker manifest lacks either `linux/amd64` or `linux/arm64`.
- Linux artifact contains a Tauri package instead of the backend + web single binary.
- Linux binary has a dynamic ELF interpreter or either architecture lacks its standalone `.bin` asset.
- Desktop packages start without bundled Mihomo core, verified GeoIP, or all 13 default rule sets.
- A run requested `sign_desktop=true`, but signing or macOS notarization is missing.
