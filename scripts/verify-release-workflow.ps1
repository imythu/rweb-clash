$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$workflowPath = Join-Path $repoRoot ".github/workflows/release.yml"

if (-not (Test-Path -LiteralPath $workflowPath -PathType Leaf)) {
  throw "Release workflow not found: $workflowPath"
}

$workflow = Get-Content -LiteralPath $workflowPath -Raw
$ciWorkflowPath = Join-Path $repoRoot ".github/workflows/ci.yml"
if (-not (Test-Path -LiteralPath $ciWorkflowPath -PathType Leaf)) {
  throw "CI workflow not found: $ciWorkflowPath"
}
$ciWorkflow = Get-Content -LiteralPath $ciWorkflowPath -Raw
$dockerfilePath = Join-Path $repoRoot "Dockerfile"
if (-not (Test-Path -LiteralPath $dockerfilePath -PathType Leaf)) {
  throw "Dockerfile not found: $dockerfilePath"
}
$dockerfile = Get-Content -LiteralPath $dockerfilePath -Raw
$tauriConfigPath = Join-Path $repoRoot "apps/desktop/src-tauri/tauri.conf.json"
if (-not (Test-Path -LiteralPath $tauriConfigPath -PathType Leaf)) {
  throw "Tauri config not found: $tauriConfigPath"
}
$tauriConfig = Get-Content -LiteralPath $tauriConfigPath -Raw
$versionScriptPath = Join-Path $repoRoot "scripts/sync-release-version.ps1"
if (-not (Test-Path -LiteralPath $versionScriptPath -PathType Leaf)) {
  throw "Release version synchronization script not found: $versionScriptPath"
}
$versionScript = Get-Content -LiteralPath $versionScriptPath -Raw

function Assert-Contains([string]$Pattern, [string]$Description) {
  if ($workflow -notmatch $Pattern) {
    throw "Release workflow is missing: $Description"
  }
}

function Assert-DockerfileContains([string]$Pattern, [string]$Description) {
  if ($dockerfile -notmatch $Pattern) {
    throw "Dockerfile is missing: $Description"
  }
}

function Get-JobSection([string]$Name) {
  $pattern = "(?ms)^  $([regex]::Escape($Name)):\s*\r?\n.*?(?=^  [A-Za-z0-9_-]+:\s*\r?\n|\z)"
  $match = [regex]::Match($workflow, $pattern)
  if (-not $match.Success) {
    throw "Release workflow job not found: $Name"
  }
  return $match.Value
}

function Get-StepSection([string]$Name) {
  $pattern = "(?ms)^      - name:\s*$([regex]::Escape($Name))\s*\r?\n.*?(?=^      - (?:name:|uses:)|^  [A-Za-z0-9_-]+:\s*\r?\n|\z)"
  $match = [regex]::Match($workflow, $pattern)
  if (-not $match.Success) {
    throw "Release workflow step not found: $Name"
  }
  return $match.Value
}

function Get-Permissions([string]$Text, [int]$Indent, [string]$Description) {
  $lines = $Text -split "`r?`n"
  $header = (" " * $Indent) + "permissions:"
  $headerIndexes = @()
  for ($index = 0; $index -lt $lines.Length; $index++) {
    if ($lines[$index].TrimEnd() -ceq $header) {
      $headerIndexes += $index
    }
  }
  if ($headerIndexes.Count -ne 1) {
    throw "$Description must contain exactly one permissions block; found $($headerIndexes.Count)."
  }

  $entryIndent = " " * ($Indent + 2)
  $permissions = @{}
  for ($index = $headerIndexes[0] + 1; $index -lt $lines.Length; $index++) {
    $line = $lines[$index]
    if ($line -notmatch "^$entryIndent([A-Za-z-]+):\s*([A-Za-z-]+)\s*$") {
      break
    }
    $permissions[$matches[1]] = $matches[2]
  }
  if ($permissions.Count -eq 0) {
    throw "$Description permissions block is empty."
  }
  return $permissions
}

function Assert-ExactPermissions([hashtable]$Actual, [hashtable]$Expected, [string]$Description) {
  if ($Actual.Count -ne $Expected.Count) {
    throw "$Description permissions are not minimal: expected $($Expected.Count) entries, found $($Actual.Count)."
  }
  foreach ($name in $Expected.Keys) {
    if (-not $Actual.ContainsKey($name) -or $Actual[$name] -ne $Expected[$name]) {
      throw "$Description permission '$name' must be '$($Expected[$name])'."
    }
  }
}

$topLevelPermissions = Get-Permissions $workflow 0 "Top-level workflow"
Assert-ExactPermissions $topLevelPermissions @{ contents = "read" } "Top-level workflow"
$dockerPermissions = Get-Permissions (Get-JobSection "docker") 4 "Docker job"
Assert-ExactPermissions $dockerPermissions @{ contents = "read"; packages = "write" } "Docker job"
$publishPermissions = Get-Permissions (Get-JobSection "publish") 4 "Publish job"
Assert-ExactPermissions $publishPermissions @{ contents = "write" } "Publish job"

foreach ($entry in @(
  @{ Name = "CI"; Content = $ciWorkflow },
  @{ Name = "Release"; Content = $workflow }
)) {
  $actionUses = [regex]::Matches(
    $entry.Content,
    "(?m)^\s*(?:-\s*)?uses:\s*([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)@([^\s#]+)"
  )
  if ($actionUses.Count -eq 0) {
    throw "$($entry.Name) workflow does not use any actions."
  }
  foreach ($actionUse in $actionUses) {
    $repository = $actionUse.Groups[1].Value
    $reference = $actionUse.Groups[2].Value
    if ($reference -notmatch "^[0-9a-f]{40}$") {
      throw "$($entry.Name) workflow action '$repository@$reference' must be pinned to a full commit SHA."
    }
  }
  $hardenedCheckouts = [regex]::Matches(
    $entry.Content,
    "(?m)^\s*- uses:\s*actions/checkout@[0-9a-f]{40}[^\r\n]*\r?\n\s*with:\s*\r?\n\s*persist-credentials:\s*false\s*$"
  ).Count
  $expectedCheckouts = if ($entry.Name -eq "Release") { 4 } else { 3 }
  if ($hardenedCheckouts -ne $expectedCheckouts) {
    throw "$($entry.Name) workflow must contain $expectedCheckouts SHA-pinned checkout steps with persisted credentials disabled; found $hardenedCheckouts."
  }
}

Assert-Contains "push:\s*\r?\n\s*branches:\s*\r?\n\s*- master" "automatic master snapshot trigger"
Assert-Contains "name:\s*Prepare release metadata" "shared release metadata job"
Assert-Contains "TZ=Asia/Shanghai date" "Beijing-time version generation"
Assert-Contains 'release_tag="snapshot-\$compact"' "automatic timestamped snapshot tag"
Assert-Contains 'release_tag="v\$compact"' "manual timestamped stable tag"
Assert-Contains 'GIT_REF.*github\.ref' "manual release source ref input"
Assert-Contains 'Stable releases must be dispatched from master' "manual stable release master guard"
Assert-Contains 'package_version="\$\(\(10#\$year\)\)\.\$month_day\.\$clock_number"' "sortable numeric package version mapping"
Assert-Contains 'macos_bundle_version="\$\{compact/-/\}"' "numeric macOS bundle version mapping"
Assert-Contains 'windows_bundle_version="\$\(\(10#\$year_short\)\)\.\$month\.\$day\.\$clock_number"' "numeric Windows MSI version mapping"
Assert-Contains 'Reject an existing timestamp tag' "minute-level tag collision guard"
Assert-Contains 'name:\s*Resolve Mihomo release tag' "single Mihomo release resolution step"
Assert-Contains 'mihomo_version:\s*\$\{\{\s*steps\.mihomo\.outputs\.version\s*\}\}' "resolved Mihomo metadata output"
Assert-Contains 'name:\s*Prepare shared runtime assets' "shared rule and GeoIP preparation job"
Assert-Contains 'name:\s*shared-runtime-assets' "shared runtime artifact"
Assert-Contains 'path:\s*packaging/cache' "shared runtime artifact download path"
Assert-Contains "package_target:\s*linux-amd64" "Linux amd64 binary target"
Assert-Contains "Sync package version" "package version synchronization step"
Assert-Contains "sync-release-version\.ps1[\s\S]*PACKAGE_VERSION" "package version synchronization command"
Assert-Contains "sync-release-version\.ps1[^\r\n]*MacosBundleVersion[^\r\n]*WindowsBundleVersion" "platform bundle version synchronization command"
$versionSyncCount = ([regex]::Matches($workflow, "run:\s*pwsh[^\r\n]*sync-release-version\.ps1")).Count
if ($versionSyncCount -ne 3) {
  throw "Release workflow must synchronize tag versions in Linux, Docker, and Tauri jobs; found $versionSyncCount invocation(s)."
}
Assert-Contains "Check release workflow structure" "CI release workflow structure check"
Assert-Contains "pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/verify-release-workflow\.ps1" "portable PowerShell workflow structure check"
if ($ciWorkflow -notmatch "(?ms)name:\s*Check release workflow structure.*?pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/verify-release-workflow\.ps1") {
  throw "CI must run the release workflow structure verifier."
}
Assert-Contains "rust_target:\s*x86_64-unknown-linux-gnu" "Linux amd64 Rust target"
Assert-Contains "package_target:\s*linux-arm64" "Linux arm64 binary target"
Assert-Contains "rust_target:\s*aarch64-unknown-linux-gnu" "Linux arm64 Rust target"
Assert-Contains "Smoke test Linux amd64 binary[\s\S]*bash scripts/release-smoke\.sh" "Linux amd64 bash smoke test"
Assert-Contains "qemu-aarch64\s+-L\s+/usr/aarch64-linux-gnu" "Linux arm64 QEMU smoke test"
Assert-Contains "cargo build -p rweb-clash-bin --features embedded-assets --release --target" "Linux embedded backend binary build"
Assert-Contains "verify-linux-archive\.sh" "Linux archive content verification"
Assert-Contains "LINUX\.md" "Linux archive platform README"
Assert-Contains "packaging/linux/install\.sh" "Linux install script syntax check"
$linuxArchiveVerifier = Get-Content -LiteralPath (Join-Path $repoRoot "scripts/verify-linux-archive.sh") -Raw
if ($linuxArchiveVerifier -notmatch "ExecStart=%h/\.local/bin/rweb-clash" -or $linuxArchiveVerifier -notmatch "Restart=on-failure") {
  throw "Linux archive verifier must check the systemd service template"
}

Assert-Contains "package_target:\s*macos-arm64" "macOS arm64 Tauri target"
Assert-Contains "rust_target:\s*aarch64-apple-darwin" "macOS arm64 Rust target"
Assert-Contains "--bundles dmg" "macOS DMG bundle"
Assert-Contains "configure-macos-signing\.sh" "macOS signing setup"
Assert-Contains "missing required macOS release secret" "macOS public release signing preflight"
$manualSigningCondition = "github\.event_name == 'workflow_dispatch' && inputs\.sign_desktop"
Assert-Contains ("name:\s*Check signed release secrets on macOS\s*\r?\n\s*if:\s*" + $manualSigningCondition + " && runner\.os == 'macOS'") "macOS signing preflight limited to opted-in manual releases"
Assert-Contains ("name:\s*Configure macOS code signing\s*\r?\n\s*if:\s*" + $manualSigningCondition + " && runner\.os == 'macOS'") "macOS signing setup limited to opted-in manual releases"
Assert-Contains ("name:\s*Build signed macOS bundles\s*\r?\n\s*if:\s*" + $manualSigningCondition + " && runner\.os == 'macOS'") "signed macOS build limited to opted-in manual releases"
if ((Get-Content -LiteralPath (Join-Path $repoRoot "scripts/configure-macos-signing.sh") -Raw) -notmatch "APPLE_SIGNING_IDENTITY=.*GITHUB_ENV") {
  throw "macOS signing setup must export APPLE_SIGNING_IDENTITY through GITHUB_ENV"
}

Assert-Contains "package_target:\s*windows-amd64" "Windows amd64 Tauri target"
Assert-Contains "rust_target:\s*x86_64-pc-windows-msvc" "Windows amd64 Rust target"
Assert-Contains "--bundles msi,nsis" "Windows MSI/NSIS bundle"
Assert-Contains "--config src-tauri/tauri\.windows\.conf\.json" "Windows explicit signing config"
Assert-Contains "configure-tauri-signing\.ps1" "Windows signing setup"
Assert-Contains "Missing Windows signing configuration" "Windows public release signing preflight"
Assert-Contains ("name:\s*Check signed release secrets on Windows\s*\r?\n\s*if:\s*" + $manualSigningCondition + " && runner\.os == 'Windows'") "Windows signing preflight limited to opted-in manual releases"
Assert-Contains ("name:\s*Configure Windows code signing\s*\r?\n\s*if:\s*" + $manualSigningCondition + " && runner\.os == 'Windows'") "Windows signing setup limited to opted-in manual releases"
Assert-Contains ("name:\s*Build signed Windows bundles\s*\r?\n\s*if:\s*" + $manualSigningCondition + " && runner\.os == 'Windows'") "signed Windows build limited to opted-in manual releases"
Assert-Contains "name:\s*Configure unsigned Windows bundle\s*\r?\n\s*if:\s*\(github\.event_name == 'push' \|\| !inputs\.sign_desktop\) && runner\.os == 'Windows'" "unsigned Windows configuration for snapshots and unsigned manual releases"
Assert-Contains "name:\s*Build unsigned desktop bundles\s*\r?\n\s*if:\s*github\.event_name == 'push' \|\| !inputs\.sign_desktop" "unsigned desktop build for snapshots and unsigned manual releases"
$windowsSignedStep = Get-StepSection "Build signed Windows bundles"
if ($windowsSignedStep -match "APPLE_") {
  throw "The signed Windows build must not receive Apple signing secrets."
}

Assert-Contains "platforms:\s*linux/amd64,linux/arm64" "Docker multiarch push"
Assert-Contains "type=raw,value=\$\{\{\s*needs\.metadata\.outputs\.release_tag\s*\}\}" "timestamped Docker version tag"
Assert-Contains "type=raw,value=snapshot,enable=\$\{\{\s*needs\.metadata\.outputs\.channel == 'snapshot'\s*\}\}" "moving Docker snapshot tag"
Assert-Contains "type=raw,value=latest,enable=\$\{\{\s*needs\.metadata\.outputs\.channel == 'stable'\s*\}\}" "Docker latest limited to manual stable releases"
Assert-Contains "verify-docker-manifest\.sh" "Docker multiarch manifest verification"
Assert-Contains "Smoke test Docker image" "Docker smoke test"
Assert-Contains "--network none" "offline Docker smoke test"
Assert-Contains "api/core/start" "Docker smoke test starts Mihomo"
Assert-Contains '\$\{GITHUB_REPOSITORY,,\}' "lowercase GHCR image name"
Assert-Contains "needs\.docker\.outputs\.image" "release body uses Docker image output"
$publishSection = Get-JobSection "publish"
if ($publishSection -match "(?m)^    if:") {
  throw "Snapshot and stable builds must both publish GitHub Releases after successful dependencies."
}
Assert-Contains "tag_name:\s*\$\{\{\s*needs\.metadata\.outputs\.release_tag\s*\}\}" "GitHub Release uses timestamped metadata tag"
Assert-Contains "prerelease:\s*\$\{\{\s*needs\.metadata\.outputs\.channel == 'snapshot'\s*\}\}" "automatic snapshots are GitHub prereleases"
Assert-Contains "make_latest:\s*\$\{\{\s*needs\.metadata\.outputs\.channel == 'stable'[^\r\n]*'true'[^\r\n]*'false'[^\r\n]*\}\}" "only manual stable releases become latest"
Assert-DockerfileContains "pnpm --dir web build" "web build stage"
Assert-DockerfileContains "package-core\.sh --target linux-amd64" "Linux amd64 Mihomo core packaging"
Assert-DockerfileContains "package-core\.sh --target linux-arm64" "Linux arm64 Mihomo core packaging"
Assert-DockerfileContains "package-runtime-assets\.sh" "GeoIP runtime asset packaging"
Assert-DockerfileContains "USE_PREPARED_RUNTIME_ASSETS" "shared release asset Docker build mode"
Assert-DockerfileContains "apt-get install[^\r\n]*jq" "structured GitHub release metadata parsing dependency"
Assert-DockerfileContains "libc6-dev-arm64-cross" "Linux arm64 cross-compilation libc headers"
Assert-DockerfileContains "cargo build -p rweb-clash-bin --features embedded-assets --release" "embedded Linux binary build"
Assert-DockerfileContains "HEALTHCHECK[\s\S]*api/setup/status" "runtime healthcheck"
Assert-DockerfileContains 'ENTRYPOINT \["/usr/local/bin/rweb-clash"\]' "runtime entrypoint"

Assert-Contains "Linux does not use Tauri" "release notes state Linux is not Tauri"
Assert-Contains "rweb-clash-linux-amd64\.tar\.gz" "Linux amd64 release archive verification"
Assert-Contains "rweb-clash-linux-arm64\.tar\.gz" "Linux arm64 release archive verification"
Assert-Contains "\*\.dmg" "macOS DMG release artifact verification"
Assert-Contains "shasum -a 256" "macOS checksum generation"
Assert-Contains "missing required artifact: Windows amd64 MSI" "Windows MSI release artifact verification"
Assert-Contains "missing required artifact: Windows amd64 NSIS" "Windows NSIS release artifact verification"
Assert-Contains "checksum mismatch" "release checksum verification"
Assert-Contains "find artifacts -type f -name.*file" "cross-artifact checksum target lookup"
Assert-Contains 'UTF8Encoding.*new\(\$false\)' "Windows checksum UTF-8 without BOM output"

Assert-Contains "package-runtime-assets\.sh" "Linux and macOS GeoIP packaging"
Assert-Contains "package-runtime-assets\.ps1" "Windows GeoIP packaging"
if ($tauriConfig -notmatch '"resources/runtime/\*"') {
  throw "Tauri bundles must include runtime GeoIP resources."
}

foreach ($requiredTarget in @(
  'Cargo\.toml',
  'apps/desktop/src-tauri/Cargo\.toml',
  'apps/desktop/src-tauri/tauri\.conf\.json',
  'apps/desktop/package\.json'
)) {
  if ($versionScript -notmatch $requiredTarget) {
    throw "Release version synchronization script is missing target: $requiredTarget"
  }
}
if ($versionScript -notmatch 'SemVer' -or $versionScript -notmatch 'StartsWith\("v"') {
  throw "Release version synchronization script must validate SemVer and strip the v tag prefix."
}
if ($versionScript -notmatch 'MacosBundleVersion' -or
    $versionScript -notmatch 'WindowsBundleVersion' -or
    $versionScript -notmatch 'bundleVersion' -or
    $versionScript -notmatch "'wix'") {
  throw "Release version synchronization script must set numeric macOS and WiX bundle versions."
}

Write-Host "Release workflow structure verified."
