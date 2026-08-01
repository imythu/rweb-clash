param(
  [Parameter(Mandatory = $true)]
  [ValidateSet("macos-arm64", "macos-aarch64", "macos-x86_64", "windows-amd64", "windows-x86_64")]
  [string]$Target
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$resourceRoot = Join-Path $repoRoot "apps/desktop/src-tauri/resources"
$ruleSourceManifest = Join-Path $repoRoot "packaging/manifests/rule-sets.toml"
$coreDir = Join-Path $resourceRoot "core"
$ruleSetDir = Join-Path $resourceRoot "rule-sets"
$runtimeDir = Join-Path $resourceRoot "runtime"

if (-not (Test-Path -LiteralPath $coreDir -PathType Container)) {
  throw "Tauri core resource directory not found: $coreDir"
}
if (-not (Test-Path -LiteralPath $ruleSetDir -PathType Container)) {
  throw "Tauri rule-set resource directory not found: $ruleSetDir"
}
if (-not (Test-Path -LiteralPath $runtimeDir -PathType Container)) {
  throw "Tauri runtime resource directory not found: $runtimeDir"
}

$isWindowsTarget = $Target.StartsWith("windows-", [System.StringComparison]::Ordinal)
$coreName = if ($isWindowsTarget) { "mihomo.exe" } else { "mihomo" }
$corePath = Join-Path $coreDir $coreName
if (-not (Test-Path -LiteralPath $corePath -PathType Leaf)) {
  throw "Tauri Mihomo resource not found for ${Target}: $corePath"
}
if ((Get-Item -LiteralPath $corePath).Length -le 0) {
  throw "Tauri Mihomo resource is empty: $corePath"
}

if ($isWindowsTarget) {
  $windowsHelper = Join-Path $resourceRoot "windows/rweb-clash-windows-helper.exe"
  if (-not (Test-Path -LiteralPath $windowsHelper -PathType Leaf) -or
      (Get-Item -LiteralPath $windowsHelper).Length -le 0) {
    throw "Windows privileged TUN helper resource is missing or empty: $windowsHelper"
  }
}

$unexpectedCore = if ($isWindowsTarget) { "mihomo" } else { "mihomo.exe" }
$unexpectedPath = Join-Path $coreDir $unexpectedCore
if (Test-Path -LiteralPath $unexpectedPath -PathType Leaf) {
  throw "Unexpected Mihomo resource for ${Target}: $unexpectedPath"
}

$ruleFiles = Get-ChildItem -LiteralPath $ruleSetDir -File -Filter "*.list"
$ruleManifestPath = Join-Path $ruleSetDir "manifest.json"
if (-not (Test-Path -LiteralPath $ruleManifestPath -PathType Leaf) -or (Get-Item -LiteralPath $ruleManifestPath).Length -le 0) {
  throw "Tauri rule-set manifest is missing or empty: $ruleManifestPath"
}

$expectedRuleIds = @(
  Get-Content -LiteralPath $ruleSourceManifest |
    ForEach-Object {
      if ($_.Trim() -match '^id\s*=\s*"([^"]+)"$') { $matches[1] }
    } |
    Sort-Object
)
if ($expectedRuleIds.Count -ne 13 -or @($expectedRuleIds | Select-Object -Unique).Count -ne 13) {
  throw "Rule-set source manifest must define exactly 13 unique IDs: $ruleSourceManifest"
}
$expectedRuleUrls = @{}
$currentRuleId = $null
foreach ($line in Get-Content -LiteralPath $ruleSourceManifest) {
  $trimmed = $line.Trim()
  if ($trimmed -eq '[[rule_sets]]') {
    $currentRuleId = $null
  } elseif ($trimmed -match '^id\s*=\s*"([^"]+)"$') {
    $currentRuleId = $matches[1]
  } elseif ($currentRuleId -and $trimmed -match '^url\s*=\s*"([^"]+)"$') {
    $expectedRuleUrls[$currentRuleId] = $matches[1]
  }
}
if ($expectedRuleUrls.Count -ne 13) {
  throw "Rule-set source manifest must define exactly 13 URLs: $ruleSourceManifest"
}

try {
  $ruleManifest = Get-Content -LiteralPath $ruleManifestPath -Raw | ConvertFrom-Json
} catch {
  throw "Tauri rule-set manifest is not valid JSON: $ruleManifestPath"
}
$ruleEntries = @($ruleManifest.ruleSets)
$sourceCommit = [string]$ruleManifest.source.commit
if ($ruleManifest.schemaVersion -ne 1 -or
    $ruleManifest.source.repository -ne 'Loyalsoldier/clash-rules' -or
    $ruleManifest.source.ref -ne 'release' -or
    $sourceCommit -notmatch '^[0-9a-f]{40}$' -or
    $ruleEntries.Count -ne 13) {
  throw "Tauri rule-set manifest must identify one release commit and contain exactly 13 entries."
}
$manifestIds = @($ruleEntries | ForEach-Object { [string]$_.id } | Sort-Object)
$manifestFiles = @($ruleEntries | ForEach-Object { [string]$_.file } | Sort-Object)
$expectedFiles = @($expectedRuleIds | ForEach-Object { "$_.list" } | Sort-Object)
$actualFiles = @($ruleFiles | ForEach-Object { $_.Name } | Sort-Object)
if (@($manifestIds | Select-Object -Unique).Count -ne 13 -or @($manifestFiles | Select-Object -Unique).Count -ne 13) {
  throw "Tauri rule-set manifest contains duplicate IDs or files."
}
if ((Compare-Object $manifestIds $expectedRuleIds) -or (Compare-Object $manifestFiles $expectedFiles) -or (Compare-Object $actualFiles $expectedFiles)) {
  throw "Tauri rule-set files and manifest must exactly match the 13 entries in $ruleSourceManifest"
}

$filesByName = @{}
foreach ($ruleFile in $ruleFiles) {
  $filesByName[$ruleFile.Name] = $ruleFile
}
foreach ($entry in $ruleEntries) {
  $id = [string]$entry.id
  $fileName = [string]$entry.file
  $sourceUrl = [string]$entry.url
  $resolvedUrl = [string]$entry.resolvedUrl
  $expectedSourceUrl = [string]$expectedRuleUrls[$id]
  if ([string]::IsNullOrWhiteSpace($id) -or [string]::IsNullOrWhiteSpace([string]$entry.name) -or
      [System.IO.Path]::GetFileName($fileName) -ne $fileName -or $fileName -ne "$id.list" -or
      $sourceUrl -ne $expectedSourceUrl -or
      $resolvedUrl -ne $sourceUrl.Replace('@release/', "@$sourceCommit/")) {
    throw "Tauri rule-set manifest contains an invalid entry for '$id'."
  }
  $expectedRuleBytes = [long]$entry.bytes
  $expectedRuleSha256 = [string]$entry.sha256
  if ($expectedRuleBytes -le 0 -or $expectedRuleBytes -gt 16777216 -or
      $expectedRuleSha256 -notmatch '^[0-9a-f]{64}$') {
    throw "Tauri rule-set manifest contains invalid size or SHA256 for $fileName."
  }
  $ruleFile = $filesByName[$fileName]
  if ($ruleFile.Length -ne $expectedRuleBytes) {
    throw "Tauri rule-set resource size mismatch for ${fileName}: expected $expectedRuleBytes, got $($ruleFile.Length)"
  }
  $actualRuleSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $ruleFile.FullName).Hash.ToLowerInvariant()
  if ($actualRuleSha256 -ne $expectedRuleSha256) {
    throw "Tauri rule-set resource SHA256 mismatch for ${fileName}: expected $expectedRuleSha256, got $actualRuleSha256"
  }
}

$runtimeAsset = Join-Path $runtimeDir "geoip.metadb"
$runtimeManifestPath = Join-Path $runtimeDir "manifest.json"
foreach ($path in @($runtimeAsset, $runtimeManifestPath)) {
  if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or (Get-Item -LiteralPath $path).Length -le 0) {
    throw "Tauri GeoIP runtime resource or manifest is missing or empty: $path"
  }
}

try {
  $runtimeManifest = Get-Content -LiteralPath $runtimeManifestPath -Raw | ConvertFrom-Json
} catch {
  throw "Tauri runtime asset manifest is not valid JSON: $runtimeManifestPath"
}
$assets = @($runtimeManifest.assets)
if ($runtimeManifest.schemaVersion -ne 1 -or $assets.Count -ne 1) {
  throw "Tauri runtime asset manifest must use schema version 1 and contain exactly one asset."
}
$asset = $assets[0]
if ($asset.id -ne "geoip" -or $asset.file -ne "geoip.metadb" -or $asset.logicalPath -ne "runtime/geoip.metadb") {
  throw "Tauri runtime asset manifest does not describe runtime/geoip.metadb."
}
if ($asset.repository -ne "MetaCubeX/meta-rules-dat" -or
    $asset.apiUrl -notmatch '^https://api\.github\.com/repos/MetaCubeX/meta-rules-dat/releases/assets/[0-9]+$' -or
    $asset.browserUrl -notmatch '^https://github\.com/MetaCubeX/meta-rules-dat/releases/download/.+/geoip\.metadb$') {
  throw "Tauri runtime asset manifest contains unexpected source URLs."
}
if ([long]$asset.releaseId -le 0 -or [long]$asset.assetId -le 0 -or
    [string]::IsNullOrWhiteSpace($asset.releaseTag) -or
    [string]::IsNullOrWhiteSpace($asset.publishedAt) -or
    [string]::IsNullOrWhiteSpace($asset.assetUpdatedAt)) {
  throw "Tauri runtime asset manifest is missing release provenance."
}
$expectedBytes = [long]$asset.bytes
if ($expectedBytes -lt 1048576 -or $expectedBytes -gt 67108864) {
  throw "Tauri runtime asset manifest contains an unreasonable size: $expectedBytes"
}
$expectedSha256 = [string]$asset.sha256
if ($expectedSha256 -notmatch '^[0-9a-f]{64}$') {
  throw "Tauri runtime asset manifest contains an invalid SHA256."
}
$actualBytes = (Get-Item -LiteralPath $runtimeAsset).Length
if ($actualBytes -ne $expectedBytes) {
  throw "Tauri GeoIP runtime resource size mismatch: expected $expectedBytes, got $actualBytes"
}
$actualSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $runtimeAsset).Hash.ToLowerInvariant()
if ($actualSha256 -ne $expectedSha256) {
  throw "Tauri GeoIP runtime resource SHA256 mismatch: expected $expectedSha256, got $actualSha256"
}

Write-Host "Verified Tauri resources for $Target at $resourceRoot"
