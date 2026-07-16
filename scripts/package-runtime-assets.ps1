$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$sourceManifest = Join-Path $repoRoot "packaging/manifests/runtime-assets.toml"
$targetDir = Join-Path $repoRoot "packaging/cache/runtime"

function Read-RuntimeAssetManifest {
  $values = @{}
  foreach ($line in Get-Content -LiteralPath $sourceManifest) {
    $trimmed = $line.Trim()
    if ($trimmed -match '^([A-Za-z0-9_]+)\s*=\s*"([^"]*)"$') {
      $values[$matches[1]] = $matches[2]
    } elseif ($trimmed -match '^([A-Za-z0-9_]+)\s*=\s*([0-9]+)$') {
      $values[$matches[1]] = [long]$matches[2]
    }
  }
  return $values
}

function Move-FileAtomically {
  param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$Destination
  )

  if (Test-Path -LiteralPath $Destination -PathType Leaf) {
    $backup = "$Destination.replace-backup.$([guid]::NewGuid().ToString('N'))"
    try {
      [System.IO.File]::Replace($Source, $Destination, $backup)
    } finally {
      Remove-Item -LiteralPath $backup -Force -ErrorAction SilentlyContinue
    }
  } else {
    [System.IO.File]::Move($Source, $Destination)
  }
}

function Invoke-AssetDownload {
  param(
    [Parameter(Mandatory = $true)][string]$Uri,
    [Parameter(Mandatory = $true)][string]$OutFile,
    [Parameter(Mandatory = $true)][hashtable]$Headers
  )

  for ($attempt = 1; $attempt -le 5; $attempt++) {
    try {
      Remove-Item -LiteralPath $OutFile -Force -ErrorAction SilentlyContinue
      Invoke-WebRequest -UseBasicParsing -Uri $Uri -Headers $Headers -OutFile $OutFile -MaximumRedirection 10 -TimeoutSec 300
      return
    } catch {
      if ($attempt -eq 5) { throw }
      Start-Sleep -Seconds 2
    }
  }
}

function Invoke-GitHubJsonRequest {
  param(
    [Parameter(Mandatory = $true)][string]$Uri,
    [Parameter(Mandatory = $true)][hashtable]$Headers
  )

  for ($attempt = 1; $attempt -le 5; $attempt++) {
    try {
      return Invoke-RestMethod -UseBasicParsing -Uri $Uri -Headers $Headers -MaximumRedirection 10 -TimeoutSec 300
    } catch {
      if ($attempt -eq 5) { throw }
      Start-Sleep -Seconds 2
    }
  }
}

$source = Read-RuntimeAssetManifest
$required = @(
  "id", "file", "logical_path", "repository", "latest_release_api",
  "asset_name", "minimum_bytes", "maximum_bytes"
)
foreach ($key in $required) {
  if (-not $source.ContainsKey($key) -or [string]::IsNullOrWhiteSpace([string]$source[$key])) {
    throw "Runtime asset manifest is missing '$key': $sourceManifest"
  }
}

$file = [string]$source.file
if ([System.IO.Path]::GetFileName($file) -ne $file -or $source.asset_name -ne $file -or $source.logical_path -ne "runtime/$file") {
  throw "Invalid runtime asset file or logical path in $sourceManifest"
}
$expectedLatestReleaseApi = "https://api.github.com/repos/$($source.repository)/releases/latest"
if ($source.latest_release_api -ne $expectedLatestReleaseApi) {
  throw "Runtime asset latest release API does not match repository $($source.repository)"
}
if ($source.minimum_bytes -le 0 -or $source.minimum_bytes -gt $source.maximum_bytes) {
  throw "Invalid runtime asset size bounds in $sourceManifest"
}

New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
$tempAsset = Join-Path $targetDir (".$file.download." + [guid]::NewGuid().ToString("N"))
$tempManifest = Join-Path $targetDir (".manifest.json." + [guid]::NewGuid().ToString("N"))
$commonHeaders = @{
  "X-GitHub-Api-Version" = "2022-11-28"
  "User-Agent" = "rweb-clash-packager"
}
if ($env:GITHUB_TOKEN) {
  $commonHeaders["Authorization"] = "Bearer $($env:GITHUB_TOKEN)"
}

try {
  $metadataHeaders = $commonHeaders.Clone()
  $metadataHeaders["Accept"] = "application/vnd.github+json"
  $release = Invoke-GitHubJsonRequest -Uri $source.latest_release_api -Headers $metadataHeaders
  if (-not $release.id -or -not $release.tag_name -or -not $release.published_at) {
    throw "GitHub latest release metadata is incomplete."
  }

  $assets = @($release.assets | Where-Object { $_.name -eq $source.asset_name })
  if ($assets.Count -ne 1) {
    throw "GitHub latest release metadata must contain exactly one $($source.asset_name) asset; found $($assets.Count)."
  }
  $asset = $assets[0]
  if (-not $asset.id -or -not $asset.url -or -not $asset.browser_download_url -or -not $asset.updated_at) {
    throw "GitHub runtime asset metadata is incomplete."
  }
  if ($asset.digest -notmatch '^sha256:([0-9a-f]{64})$') {
    throw "GitHub runtime asset metadata does not contain a SHA256 digest."
  }
  $expectedSha256 = $matches[1]
  $expectedBytes = [long]$asset.size
  $expectedAssetApiUrl = "https://api.github.com/repos/$($source.repository)/releases/assets/$($asset.id)"
  $browserUrlPrefix = "https://github.com/$($source.repository)/releases/download/"
  if ($asset.url -ne $expectedAssetApiUrl -or
      -not $asset.browser_download_url.StartsWith($browserUrlPrefix, [System.StringComparison]::Ordinal) -or
      -not $asset.browser_download_url.EndsWith("/$($source.asset_name)", [System.StringComparison]::Ordinal)) {
    throw "GitHub runtime asset URLs do not match repository $($source.repository) and asset $($asset.id)"
  }
  if ($expectedBytes -lt $source.minimum_bytes -or $expectedBytes -gt $source.maximum_bytes) {
    throw "GitHub runtime asset size is outside the allowed range: $expectedBytes bytes"
  }

  $assetHeaders = $commonHeaders.Clone()
  $assetHeaders["Accept"] = "application/octet-stream"
  Invoke-AssetDownload -Uri $asset.url -OutFile $tempAsset -Headers $assetHeaders

  $actualBytes = (Get-Item -LiteralPath $tempAsset).Length
  if ($actualBytes -lt $source.minimum_bytes -or $actualBytes -gt $source.maximum_bytes) {
    throw "Runtime asset size is outside the allowed range: $actualBytes bytes"
  }
  if ($actualBytes -ne $expectedBytes) {
    throw "Runtime asset size mismatch: expected $expectedBytes, got $actualBytes"
  }

  $actualSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $tempAsset).Hash.ToLowerInvariant()
  if ($actualSha256 -ne $expectedSha256) {
    throw "Runtime asset SHA256 mismatch: expected $expectedSha256, got $actualSha256"
  }

  $output = [ordered]@{
    schemaVersion = 1
    generatedAt = [DateTimeOffset]::UtcNow.ToString("o")
    assets = @(
      [ordered]@{
        id = $source.id
        file = $file
        logicalPath = $source.logical_path
        repository = $source.repository
        releaseId = [long]$release.id
        releaseTag = $release.tag_name
        assetId = [long]$asset.id
        apiUrl = $asset.url
        browserUrl = $asset.browser_download_url
        publishedAt = ([DateTimeOffset]$release.published_at).ToString("o")
        assetUpdatedAt = ([DateTimeOffset]$asset.updated_at).ToString("o")
        bytes = $actualBytes
        sha256 = $actualSha256
      }
    )
  }
  $json = $output | ConvertTo-Json -Depth 6
  [System.IO.File]::WriteAllText($tempManifest, "$json`n", [System.Text.UTF8Encoding]::new($false))

  Move-FileAtomically -Source $tempAsset -Destination (Join-Path $targetDir $file)
  Move-FileAtomically -Source $tempManifest -Destination (Join-Path $targetDir "manifest.json")
  Write-Host "Downloaded runtime asset $($source.logical_path) from release $($release.tag_name) (asset $($asset.id))"
} finally {
  Remove-Item -LiteralPath $tempAsset, $tempManifest -Force -ErrorAction SilentlyContinue
}
