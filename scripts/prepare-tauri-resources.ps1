param(
  [Parameter(Mandatory = $true)]
  [string]$Target
)

$ErrorActionPreference = "Stop"

function Invoke-Native {
  param(
    [Parameter(Mandatory = $true)]
    [scriptblock]$Command
  )
  & $Command
  if ($LASTEXITCODE -ne 0) {
    throw "Command failed with exit code $LASTEXITCODE."
  }
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$resourceRoot = Join-Path $repoRoot "apps/desktop/src-tauri/resources"
$coreSource = Join-Path $repoRoot "packaging/cache/cores/$Target"
$ruleSetSource = Join-Path $repoRoot "packaging/cache/rule-sets"
$runtimeSource = Join-Path $repoRoot "packaging/cache/runtime"
$coreDest = Join-Path $resourceRoot "core"
$ruleSetDest = Join-Path $resourceRoot "rule-sets"
$runtimeDest = Join-Path $resourceRoot "runtime"
$windowsHelperDest = Join-Path $resourceRoot "windows"

if (-not (Test-Path -LiteralPath $coreSource)) {
  throw "Core cache not found: $coreSource"
}
if (-not (Test-Path -LiteralPath $ruleSetSource)) {
  throw "Rule-set cache not found: $ruleSetSource"
}
$ruleManifest = Join-Path $ruleSetSource "manifest.json"
if (-not (Test-Path -LiteralPath $ruleManifest -PathType Leaf) -or (Get-Item -LiteralPath $ruleManifest).Length -le 0) {
  throw "Rule-set cache manifest not found or empty: $ruleManifest"
}
$runtimeAsset = Join-Path $runtimeSource "geoip.metadb"
$runtimeManifest = Join-Path $runtimeSource "manifest.json"
foreach ($path in @($runtimeAsset, $runtimeManifest)) {
  if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or (Get-Item -LiteralPath $path).Length -le 0) {
    throw "Verified runtime asset cache file not found or empty: $path"
  }
}

Remove-Item -LiteralPath $resourceRoot -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $coreDest, $ruleSetDest, $runtimeDest | Out-Null

$coreFiles = Get-ChildItem -LiteralPath $coreSource -File |
  Where-Object { $_.Name -eq "mihomo" -or $_.Name -eq "mihomo.exe" }
if (-not $coreFiles) {
  throw "No Mihomo binary found in $coreSource"
}
$coreFiles | Copy-Item -Destination $coreDest -Force

$ruleFiles = Get-ChildItem -LiteralPath $ruleSetSource -File -Filter "*.list"
if (-not $ruleFiles) {
  throw "No rule-set list files found in $ruleSetSource"
}
$ruleFiles | Copy-Item -Destination $ruleSetDest -Force

Copy-Item -LiteralPath $ruleManifest -Destination $ruleSetDest -Force
Copy-Item -LiteralPath $runtimeAsset, $runtimeManifest -Destination $runtimeDest -Force

if ($Target.StartsWith("windows-", [System.StringComparison]::Ordinal)) {
  $helperTarget = "x86_64-pc-windows-msvc"
  $helperManifest = Join-Path $repoRoot "crates/rweb-clash-windows-helper/Cargo.toml"
  Invoke-Native {
    cargo build --manifest-path $helperManifest --release --locked --target $helperTarget
  }
  $helperPath = Join-Path $repoRoot "target/$helperTarget/release/rweb-clash-windows-helper.exe"
  if (-not (Test-Path -LiteralPath $helperPath -PathType Leaf)) {
    throw "Windows TUN helper binary not found: $helperPath"
  }
  New-Item -ItemType Directory -Force -Path $windowsHelperDest | Out-Null
  Copy-Item -LiteralPath $helperPath -Destination $windowsHelperDest -Force
}

Write-Host "Prepared Tauri resources for $Target at $resourceRoot"
