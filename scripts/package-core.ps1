param(
  [Parameter(Mandatory = $true)]
  [ValidateSet("linux-amd64", "linux-arm64", "linux-x86_64", "linux-aarch64", "windows-amd64", "windows-x86_64", "macos-arm64", "macos-aarch64", "macos-x86_64")]
  [string]$Target,
  [string]$Version = "latest"
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$downloadBase = "https://github.com/MetaCubeX/mihomo/releases/download"
$targetDir = Join-Path $repoRoot "packaging/cache/cores/$Target"
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("rweb-clash-core-" + [guid]::NewGuid().ToString("N"))

function Invoke-GitHubRequest([string]$Uri, [hashtable]$Headers) {
  for ($attempt = 1; $attempt -le 5; $attempt++) {
    try {
      return Invoke-RestMethod -UseBasicParsing -Uri $Uri -Headers $Headers -MaximumRedirection 10 -TimeoutSec 300
    } catch {
      if ($attempt -eq 5) { throw }
      Start-Sleep -Seconds ([Math]::Min($attempt * 2, 10))
    }
  }
}

function Invoke-AssetDownload([string]$Uri, [string]$OutFile, [hashtable]$Headers) {
  for ($attempt = 1; $attempt -le 5; $attempt++) {
    try {
      Remove-Item -LiteralPath $OutFile -Force -ErrorAction SilentlyContinue
      Invoke-WebRequest -UseBasicParsing -Uri $Uri -Headers $Headers -OutFile $OutFile -MaximumRedirection 10 -TimeoutSec 300
      return
    } catch {
      if ($attempt -eq 5) { throw }
      Start-Sleep -Seconds ([Math]::Min($attempt * 2, 10))
    }
  }
}

function Get-TargetSpec([string]$Target) {
  switch ($Target) {
    "windows-amd64" { return @{ Os = "windows"; Archive = "zip"; Binary = "mihomo.exe"; Arch = @("amd64", "amd64-compatible") } }
    "windows-x86_64" { return @{ Os = "windows"; Archive = "zip"; Binary = "mihomo.exe"; Arch = @("amd64", "amd64-compatible") } }
    "macos-arm64" { return @{ Os = "darwin"; Archive = "gz"; Binary = "mihomo"; Arch = @("arm64") } }
    "macos-aarch64" { return @{ Os = "darwin"; Archive = "gz"; Binary = "mihomo"; Arch = @("arm64") } }
    "macos-x86_64" { return @{ Os = "darwin"; Archive = "gz"; Binary = "mihomo"; Arch = @("amd64", "amd64-compatible") } }
    "linux-amd64" { return @{ Os = "linux"; Archive = "gz"; Binary = "mihomo"; Arch = @("amd64", "amd64-compatible") } }
    "linux-x86_64" { return @{ Os = "linux"; Archive = "gz"; Binary = "mihomo"; Arch = @("amd64", "amd64-compatible") } }
    "linux-arm64" { return @{ Os = "linux"; Archive = "gz"; Binary = "mihomo"; Arch = @("arm64") } }
    "linux-aarch64" { return @{ Os = "linux"; Archive = "gz"; Binary = "mihomo"; Arch = @("arm64") } }
  }
}

function Expand-CoreArchive($archivePath, $targetBinary, $archiveKind) {
  if ($archiveKind -eq "zip") {
    $extractDir = Join-Path $tempDir "extract"
    Expand-Archive -LiteralPath $archivePath -DestinationPath $extractDir -Force
    $binary = Get-ChildItem -Path $extractDir -Recurse -File |
      Where-Object { $_.Name.ToLowerInvariant().StartsWith("mihomo") -and $_.Extension.ToLowerInvariant() -eq ".exe" } |
      Select-Object -First 1
    if (-not $binary) { throw "Downloaded archive does not contain mihomo.exe." }
    Copy-Item -LiteralPath $binary.FullName -Destination $targetBinary -Force
    return
  }

  $source = [System.IO.File]::OpenRead($archivePath)
  try {
    $gzip = [System.IO.Compression.GzipStream]::new($source, [System.IO.Compression.CompressionMode]::Decompress)
    try {
      $target = [System.IO.File]::Create($targetBinary)
      try { $gzip.CopyTo($target) } finally { $target.Dispose() }
    } finally {
      $gzip.Dispose()
    }
  } finally {
    $source.Dispose()
  }
}

try {
  $spec = Get-TargetSpec $Target
  if ($Version -ne "latest" -and $Version -notmatch '^[0-9A-Za-z._-]+$') {
    throw "Invalid Mihomo release tag: $Version"
  }
  $commonHeaders = @{
    "X-GitHub-Api-Version" = "2022-11-28"
    "User-Agent" = "rweb-clash-packager"
  }
  if ($env:GITHUB_TOKEN) {
    $commonHeaders["Authorization"] = "Bearer $($env:GITHUB_TOKEN)"
  }
  $metadataHeaders = $commonHeaders.Clone()
  $metadataHeaders["Accept"] = "application/vnd.github+json"
  $metadataUrl = if ($Version -eq "latest") {
    "https://api.github.com/repos/MetaCubeX/mihomo/releases/latest"
  } else {
    "https://api.github.com/repos/MetaCubeX/mihomo/releases/tags/$([uri]::EscapeDataString($Version))"
  }
  $release = Invoke-GitHubRequest -Uri $metadataUrl -Headers $metadataHeaders
  $tag = [string]$release.tag_name
  if ($tag -notmatch '^[0-9A-Za-z._-]+$' -or -not $release.id -or -not $release.published_at) {
    throw "GitHub Mihomo release metadata is incomplete."
  }
  if ($Version -ne "latest" -and $tag -ne $Version) {
    throw "Requested Mihomo tag $Version resolved to unexpected tag $tag."
  }
  New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
  New-Item -ItemType Directory -Force -Path $tempDir | Out-Null

  $downloaded = $null
  $downloadErrors = @()
  foreach ($arch in $spec.Arch) {
    $assetName = "mihomo-$($spec.Os)-$arch-$tag.$($spec.Archive)"
    $assets = @($release.assets | Where-Object { $_.name -eq $assetName })
    if ($assets.Count -ne 1) {
      $downloadErrors += "$assetName (metadata matches: $($assets.Count))"
      continue
    }
    $asset = $assets[0]
    $assetUrl = [string]$asset.browser_download_url
    $expectedApiUrl = "https://api.github.com/repos/MetaCubeX/mihomo/releases/assets/$($asset.id)"
    if ($asset.url -ne $expectedApiUrl -or $assetUrl -ne "$downloadBase/$tag/$assetName" -or
        $asset.digest -notmatch '^sha256:([0-9a-f]{64})$') {
      throw "Invalid GitHub asset metadata for $assetName."
    }
    $expectedSha256 = $matches[1]
    $expectedBytes = [long]$asset.size
    if ($expectedBytes -lt 1048576 -or $expectedBytes -gt 134217728) {
      throw "Mihomo archive size is outside the allowed range: $expectedBytes bytes."
    }
    $archivePath = Join-Path $tempDir $assetName
    try {
      $assetHeaders = $commonHeaders.Clone()
      $assetHeaders["Accept"] = "application/octet-stream"
      Invoke-AssetDownload -Uri $asset.url -OutFile $archivePath -Headers $assetHeaders
      $actualBytes = (Get-Item -LiteralPath $archivePath).Length
      $actualSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
      if ($actualBytes -ne $expectedBytes -or $actualSha256 -ne $expectedSha256) {
        throw "Mihomo archive verification failed for $assetName."
      }
      $downloaded = @{
        Name = $assetName
        Url = $assetUrl
        Path = $archivePath
        AssetId = [long]$asset.id
        AssetUpdatedAt = ([DateTimeOffset]$asset.updated_at).ToString("o")
        ArchiveSha256 = $actualSha256
        ArchiveBytes = $actualBytes
      }
      break
    } catch {
      $downloadErrors += "$assetName`: $($_.Exception.Message)"
      if (Test-Path -LiteralPath $archivePath) { Remove-Item -LiteralPath $archivePath -Force }
    }
  }
  if (-not $downloaded) {
    throw "No direct mihomo asset matched $Target for $tag. Tried: $($downloadErrors -join '; ')"
  }

  $targetBinary = Join-Path $targetDir $spec.Binary
  Expand-CoreArchive $downloaded.Path $targetBinary $spec.Archive
  $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $targetBinary).Hash.ToLowerInvariant()
  $manifest = @{
    target = $Target
    version = $tag
    releaseId = [long]$release.id
    publishedAt = ([DateTimeOffset]$release.published_at).ToString("o")
    assetId = $downloaded.AssetId
    assetUpdatedAt = $downloaded.AssetUpdatedAt
    asset = $downloaded.Name
    url = $downloaded.Url
    archiveSha256 = $downloaded.ArchiveSha256
    archiveBytes = $downloaded.ArchiveBytes
    binary = $spec.Binary
    sha256 = $hash
    bytes = (Get-Item -LiteralPath $targetBinary).Length
    generatedAt = [DateTimeOffset]::UtcNow.ToString("o")
  } | ConvertTo-Json -Depth 5
  [System.IO.File]::WriteAllText(
    (Join-Path $targetDir "manifest.json"),
    $manifest + [Environment]::NewLine,
    [System.Text.UTF8Encoding]::new($false)
  )
  Write-Host "Downloaded $tag ($($downloaded.Name)) to $targetBinary"
} finally {
  if (Test-Path -LiteralPath $tempDir) {
    Remove-Item -LiteralPath $tempDir -Recurse -Force
  }
}
