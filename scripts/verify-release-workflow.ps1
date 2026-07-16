$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$workflowPath = Join-Path $repoRoot ".github/workflows/release.yml"

if (-not (Test-Path -LiteralPath $workflowPath -PathType Leaf)) {
  throw "Release workflow not found: $workflowPath"
}

$workflow = Get-Content -LiteralPath $workflowPath -Raw
$dockerfilePath = Join-Path $repoRoot "Dockerfile"
if (-not (Test-Path -LiteralPath $dockerfilePath -PathType Leaf)) {
  throw "Dockerfile not found: $dockerfilePath"
}
$dockerfile = Get-Content -LiteralPath $dockerfilePath -Raw
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

Assert-Contains "package_target:\s*linux-amd64" "Linux amd64 binary target"
Assert-Contains "Sync release version from tag" "tag version synchronization step"
Assert-Contains "sync-release-version\.ps1[\s\S]*RELEASE_TAG" "release version synchronization command"
$versionSyncCount = ([regex]::Matches($workflow, "run:\s*pwsh[^\r\n]*sync-release-version\.ps1")).Count
if ($versionSyncCount -ne 3) {
  throw "Release workflow must synchronize tag versions in Linux, Docker, and Tauri jobs; found $versionSyncCount invocation(s)."
}
$versionSyncConditionCount = ([regex]::Matches(
  $workflow,
  "if:\s*github\.event_name == 'push' && startsWith\(github\.ref, 'refs/tags/v'\)"
)).Count
if ($versionSyncConditionCount -ne 3) {
  throw "Release version synchronization must run only for pushed v* tags in all three build jobs."
}
Assert-Contains "Check release workflow structure" "CI release workflow structure check"
Assert-Contains "pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/verify-release-workflow\.ps1" "portable PowerShell workflow structure check"
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
if ((Get-Content -LiteralPath (Join-Path $repoRoot "scripts/configure-macos-signing.sh") -Raw) -notmatch "APPLE_SIGNING_IDENTITY=.*GITHUB_ENV") {
  throw "macOS signing setup must export APPLE_SIGNING_IDENTITY through GITHUB_ENV"
}

Assert-Contains "package_target:\s*windows-amd64" "Windows amd64 Tauri target"
Assert-Contains "rust_target:\s*x86_64-pc-windows-msvc" "Windows amd64 Rust target"
Assert-Contains "--bundles msi,nsis" "Windows MSI/NSIS bundle"
Assert-Contains "--config src-tauri/tauri\.windows\.conf\.json" "Windows explicit signing config"
Assert-Contains "configure-tauri-signing\.ps1" "Windows signing setup"
Assert-Contains "Missing Windows signing configuration" "Windows public release signing preflight"

Assert-Contains "platforms:\s*linux/amd64,linux/arm64" "Docker multiarch push"
$latestTagLines = @($workflow -split "`r?`n" | Where-Object { $_ -match 'type=raw,value=latest' })
if ($latestTagLines.Count -ne 1 `
  -or $latestTagLines[0] -notmatch "refs/tags/v" `
  -or $latestTagLines[0] -notmatch "github\.event_name == 'push'" `
  -or $latestTagLines[0] -notmatch "!contains\(github\.ref_name, '-'\)" `
  -or $latestTagLines[0] -notmatch "workflow_dispatch" `
  -or $latestTagLines[0] -notmatch "repository\.default_branch") {
  throw "Docker latest must be published for stable v* tags and only for default-branch workflow_dispatch runs."
}
Assert-Contains "verify-docker-manifest\.sh" "Docker multiarch manifest verification"
Assert-Contains "Smoke test Docker image" "Docker smoke test"
Assert-Contains '\$\{GITHUB_REPOSITORY,,\}' "lowercase GHCR image name"
Assert-Contains "needs\.docker\.outputs\.image" "release body uses Docker image output"
Assert-Contains "prerelease:\s*\$\{\{\s*contains\(github\.ref_name, '-'\)\s*\}\}" "prerelease tags are marked as GitHub prereleases"
Assert-Contains "make_latest:\s*\$\{\{\s*contains\(github\.ref_name, '-'\)[^\r\n]*'false'[^\r\n]*'true'[^\r\n]*\}\}" "only stable tags become the latest GitHub release"
Assert-DockerfileContains "pnpm --dir web build" "web build stage"
Assert-DockerfileContains "package-core\.sh --target linux-amd64" "Linux amd64 Mihomo core packaging"
Assert-DockerfileContains "package-core\.sh --target linux-arm64" "Linux arm64 Mihomo core packaging"
Assert-DockerfileContains "cargo build -p rweb-clash-bin --features embedded-assets --release" "embedded Linux binary build"
Assert-DockerfileContains "HEALTHCHECK[\s\S]*api/setup/status" "runtime healthcheck"
Assert-DockerfileContains 'ENTRYPOINT \["/usr/local/bin/rweb-clash"\]' "runtime entrypoint"

Assert-Contains "Linux does not use Tauri" "release notes state Linux is not Tauri"
Assert-Contains "rweb-clash-linux-amd64\.tar\.gz" "Linux amd64 release archive verification"
Assert-Contains "rweb-clash-linux-arm64\.tar\.gz" "Linux arm64 release archive verification"
Assert-Contains "\*\.dmg" "macOS DMG release artifact verification"
Assert-Contains "shasum -a 256" "macOS checksum generation"
Assert-Contains "\*\.msi.*\*\.exe|\*\.exe.*\*\.msi" "Windows installer release artifact verification"
Assert-Contains "checksum mismatch" "release checksum verification"
Assert-Contains "find artifacts -type f -name.*file" "cross-artifact checksum target lookup"

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

Write-Host "Release workflow structure verified."
