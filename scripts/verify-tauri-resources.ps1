param(
  [Parameter(Mandatory = $true)]
  [ValidateSet("macos-arm64", "windows-amd64")]
  [string]$Target
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$resourceRoot = Join-Path $repoRoot "apps/desktop/src-tauri/resources"
$coreDir = Join-Path $resourceRoot "core"
$ruleSetDir = Join-Path $resourceRoot "rule-sets"

if (-not (Test-Path -LiteralPath $coreDir -PathType Container)) {
  throw "Tauri core resource directory not found: $coreDir"
}
if (-not (Test-Path -LiteralPath $ruleSetDir -PathType Container)) {
  throw "Tauri rule-set resource directory not found: $ruleSetDir"
}

$coreName = if ($Target -eq "windows-amd64") { "mihomo.exe" } else { "mihomo" }
$corePath = Join-Path $coreDir $coreName
if (-not (Test-Path -LiteralPath $corePath -PathType Leaf)) {
  throw "Tauri Mihomo resource not found for ${Target}: $corePath"
}
if ((Get-Item -LiteralPath $corePath).Length -le 0) {
  throw "Tauri Mihomo resource is empty: $corePath"
}

$unexpectedCore = if ($Target -eq "windows-amd64") { "mihomo" } else { "mihomo.exe" }
$unexpectedPath = Join-Path $coreDir $unexpectedCore
if (Test-Path -LiteralPath $unexpectedPath -PathType Leaf) {
  throw "Unexpected Mihomo resource for ${Target}: $unexpectedPath"
}

$ruleFiles = Get-ChildItem -LiteralPath $ruleSetDir -File -Filter "*.list"
if (-not $ruleFiles) {
  throw "No Tauri rule-set list files found in $ruleSetDir"
}
foreach ($file in $ruleFiles) {
  if ($file.Length -le 0) {
    throw "Tauri rule-set resource is empty: $($file.FullName)"
  }
}

Write-Host "Verified Tauri resources for $Target at $resourceRoot"
