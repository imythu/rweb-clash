param(
  [Parameter(Mandatory = $true)]
  [string]$Target
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$resourceRoot = Join-Path $repoRoot "apps/desktop/src-tauri/resources"
$coreSource = Join-Path $repoRoot "packaging/cache/cores/$Target"
$ruleSetSource = Join-Path $repoRoot "packaging/cache/rule-sets"
$coreDest = Join-Path $resourceRoot "core"
$ruleSetDest = Join-Path $resourceRoot "rule-sets"

if (-not (Test-Path -LiteralPath $coreSource)) {
  throw "Core cache not found: $coreSource"
}
if (-not (Test-Path -LiteralPath $ruleSetSource)) {
  throw "Rule-set cache not found: $ruleSetSource"
}

Remove-Item -LiteralPath $resourceRoot -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $coreDest, $ruleSetDest | Out-Null

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

$ruleManifest = Join-Path $ruleSetSource "manifest.json"
if (Test-Path -LiteralPath $ruleManifest) {
  Copy-Item -LiteralPath $ruleManifest -Destination $ruleSetDest -Force
}

Write-Host "Prepared Tauri resources for $Target at $resourceRoot"
