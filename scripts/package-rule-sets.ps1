$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$manifestPath = Join-Path $repoRoot "packaging/manifests/rule-sets.toml"
$targetDir = Join-Path $repoRoot "packaging/cache/rule-sets"
$cacheRoot = Join-Path $repoRoot "packaging/cache"
$stagingRoot = Join-Path $cacheRoot (".rule-sets-" + [guid]::NewGuid().ToString("N"))
$stagingDir = Join-Path $stagingRoot "rule-sets"
$manifestOut = Join-Path $stagingDir "manifest.json"
$sourceRepository = "Loyalsoldier/clash-rules"
$sourceRef = "release"
$sourceUrlPrefix = "https://cdn.jsdelivr.net/gh/$sourceRepository@$sourceRef/"
$maximumRuleBytes = 16777216

New-Item -ItemType Directory -Force -Path $stagingDir | Out-Null

$items = @()
$current = $null
foreach ($line in Get-Content -LiteralPath $manifestPath) {
  $trimmed = $line.Trim()
  if ($trimmed -eq "[[rule_sets]]") {
    if ($current) { $items += $current }
    $current = [ordered]@{}
    continue
  }
  if ($current -and $trimmed -match '^([A-Za-z0-9_-]+)\s*=\s*"([^"]*)"$') {
    $current[$matches[1]] = $matches[2]
  }
}
if ($current) { $items += $current }

if ($items.Count -ne 13) {
  throw "Expected exactly 13 builtin rule sets, found $($items.Count)."
}

try {
  $apiHeaders = @{
    "Accept" = "application/vnd.github+json"
    "X-GitHub-Api-Version" = "2022-11-28"
    "User-Agent" = "rweb-clash-packager"
  }
  if ($env:GITHUB_TOKEN) {
    $apiHeaders["Authorization"] = "Bearer $($env:GITHUB_TOKEN)"
  }
  $commitMetadata = $null
  for ($attempt = 1; $attempt -le 5; $attempt++) {
    try {
      $commitMetadata = Invoke-RestMethod `
        -UseBasicParsing `
        -Uri "https://api.github.com/repos/$sourceRepository/commits/$sourceRef" `
        -Headers $apiHeaders `
        -MaximumRedirection 10 `
        -TimeoutSec 180
      break
    } catch {
      if ($attempt -eq 5) { throw }
      Start-Sleep -Seconds ([Math]::Min($attempt * 2, 10))
    }
  }
  $sourceCommit = [string]$commitMetadata.sha
  if ($sourceCommit -notmatch '^[0-9a-f]{40}$') {
    throw "Failed to resolve $sourceRepository@$sourceRef to a Git commit."
  }

  $outputs = @()
  foreach ($item in $items) {
    $sourceUrl = [string]$item.url
    if (-not $sourceUrl.StartsWith($sourceUrlPrefix, [System.StringComparison]::Ordinal)) {
      throw "Unexpected rule-set source URL: $sourceUrl"
    }
    $resolvedUrl = $sourceUrl.Replace("@$sourceRef/", "@$sourceCommit/")
    $fileName = "$($item.id).list"
    $target = Join-Path $stagingDir $fileName
    $downloaded = $false
    for ($attempt = 1; $attempt -le 5; $attempt++) {
      try {
        Invoke-WebRequest `
          -UseBasicParsing `
          -Uri $resolvedUrl `
          -Headers @{ "User-Agent" = "rweb-clash" } `
          -OutFile $target `
          -TimeoutSec 180
        $downloaded = $true
        break
      } catch {
        Remove-Item -LiteralPath $target -Force -ErrorAction SilentlyContinue
        if ($attempt -eq 5) { throw }
        Start-Sleep -Seconds ([Math]::Min($attempt * 2, 10))
      }
    }
    if (-not $downloaded -or -not (Test-Path -LiteralPath $target -PathType Leaf)) {
      throw "Failed to download rule set $($item.name) from $resolvedUrl."
    }
    $file = Get-Item -LiteralPath $target
    if ($file.Length -le 0) {
      throw "Downloaded rule set is empty: $($item.name) ($resolvedUrl)."
    }
    if ($file.Length -gt $maximumRuleBytes) {
      throw "Downloaded rule set exceeds $maximumRuleBytes bytes: $($item.name)."
    }
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $target).Hash.ToLowerInvariant()
    $outputs += [ordered]@{
      id = $item.id
      name = $item.name
      url = $sourceUrl
      resolvedUrl = $resolvedUrl
      file = $fileName
      sha256 = $hash
      bytes = $file.Length
    }
    Write-Host "Downloaded $($item.name) to $target"
  }

  $downloadedFiles = @(Get-ChildItem -LiteralPath $stagingDir -File -Filter "*.list")
  if ($downloadedFiles.Count -ne 13) {
    throw "Expected exactly 13 downloaded rule files, found $($downloadedFiles.Count)."
  }

  $manifestJson = @{
    schemaVersion = 1
    generatedAt = [DateTimeOffset]::UtcNow.ToString("o")
    source = [ordered]@{
      repository = $sourceRepository
      ref = $sourceRef
      commit = $sourceCommit
    }
    ruleSets = $outputs
  } | ConvertTo-Json -Depth 5
  [System.IO.File]::WriteAllText(
    $manifestOut,
    $manifestJson + [Environment]::NewLine,
    [System.Text.UTF8Encoding]::new($false)
  )

  if (Test-Path -LiteralPath $targetDir) {
    Remove-Item -LiteralPath $targetDir -Recurse -Force
  }
  Move-Item -LiteralPath $stagingDir -Destination $targetDir
  Write-Host "Wrote $(Join-Path $targetDir 'manifest.json') from $sourceRepository@$sourceCommit"
} finally {
  Remove-Item -LiteralPath $stagingRoot -Recurse -Force -ErrorAction SilentlyContinue
}
