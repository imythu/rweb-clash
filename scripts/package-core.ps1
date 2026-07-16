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

function Resolve-LatestTag {
  $release = Invoke-RestMethod `
    -Uri "https://api.github.com/repos/MetaCubeX/mihomo/releases/latest" `
    -Headers @{ "User-Agent" = "rweb-clash" }
  if (-not $release.tag_name) {
    throw "GitHub latest release did not return tag_name."
  }
  return $release.tag_name
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
  $tag = if ($Version -eq "latest") { Resolve-LatestTag } else { $Version }
  New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
  New-Item -ItemType Directory -Force -Path $tempDir | Out-Null

  $downloaded = $null
  $downloadErrors = @()
  foreach ($arch in $spec.Arch) {
    $assetName = "mihomo-$($spec.Os)-$arch-$tag.$($spec.Archive)"
    $assetUrl = "$downloadBase/$tag/$assetName"
    $archivePath = Join-Path $tempDir $assetName
    try {
      Invoke-WebRequest -Uri $assetUrl -Headers @{ "User-Agent" = "rweb-clash" } -OutFile $archivePath
      $downloaded = @{ Name = $assetName; Url = $assetUrl; Path = $archivePath }
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
  @{
    target = $Target
    version = $tag
    asset = $downloaded.Name
    url = $downloaded.Url
    binary = $spec.Binary
    sha256 = $hash
    bytes = (Get-Item -LiteralPath $targetBinary).Length
    generatedAt = [DateTimeOffset]::UtcNow.ToString("o")
  } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $targetDir "manifest.json") -Encoding utf8
  Write-Host "Downloaded $tag ($($downloaded.Name)) to $targetBinary"
} finally {
  if (Test-Path -LiteralPath $tempDir) {
    Remove-Item -LiteralPath $tempDir -Recurse -Force
  }
}
