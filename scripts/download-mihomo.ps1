$ErrorActionPreference = "Stop"

$latestUrl = "https://github.com/MetaCubeX/mihomo/releases/latest"
$downloadBase = "https://github.com/MetaCubeX/mihomo/releases/download"
$targetDir = Join-Path (Get-Location) "cache-core"
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("mihomo-download-" + [guid]::NewGuid().ToString("N"))

function Resolve-LatestTag {
  $handler = [System.Net.Http.HttpClientHandler]::new()
  $handler.AllowAutoRedirect = $false
  $client = [System.Net.Http.HttpClient]::new($handler)
  try {
    $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Head, $latestUrl)
    $response = $client.Send($request)
    $location = $response.Headers.Location
    if (-not $location) {
      throw "GitHub latest release did not return a redirect."
    }
    if (-not $location.IsAbsoluteUri) {
      $location = [System.Uri]::new([System.Uri]$latestUrl, $location)
    }
    return ($location.AbsoluteUri.TrimEnd("/") -split "/")[-1]
  } finally {
    $client.Dispose()
    $handler.Dispose()
  }
}

function Get-CurrentPlatform {
  if ([System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)) {
    return @{
      OsToken = "windows"
      ArchiveExt = "zip"
      BinaryName = "mihomo.exe"
    }
  }

  if ([System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::OSX)) {
    return @{
      OsToken = "darwin"
      ArchiveExt = "gz"
      BinaryName = "mihomo"
    }
  }

  if ([System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Linux)) {
    return @{
      OsToken = "linux"
      ArchiveExt = "gz"
      BinaryName = "mihomo"
    }
  }

  throw "Unsupported OS: $([System.Runtime.InteropServices.RuntimeInformation]::OSDescription)"
}

function Get-ArchCandidates {
  $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
  switch ($arch) {
    "x64" { return @("amd64", "amd64-compatible") }
    "x86" { return @("386") }
    "arm64" { return @("arm64") }
    "arm" { return @("armv7", "arm") }
    default { throw "Unsupported architecture: $arch" }
  }
}

function Get-AssetNames([string]$tag, $platform) {
  $names = @()
  foreach ($arch in Get-ArchCandidates) {
    $names += "mihomo-$($platform.OsToken)-$arch-$tag.$($platform.ArchiveExt)"
  }
  return $names
}

function Download-FirstAvailableAsset([string]$tag, [string[]]$assetNames, [string]$destinationDir) {
  foreach ($assetName in $assetNames) {
    $assetUrl = "$downloadBase/$tag/$assetName"
    $archivePath = Join-Path $destinationDir $assetName
    try {
      Invoke-WebRequest -Uri $assetUrl -Headers @{ "User-Agent" = "rweb-clash" } -OutFile $archivePath
      return @{
        Name = $assetName
        Path = $archivePath
        Url = $assetUrl
      }
    } catch {
      if (Test-Path -LiteralPath $archivePath) {
        Remove-Item -LiteralPath $archivePath -Force
      }
    }
  }

  throw "No direct mihomo asset matched this build machine. Tried: $($assetNames -join ', ')"
}

function Expand-MihomoArchive($asset, [string]$targetBinary, $platform) {
  if ($platform.ArchiveExt -eq "zip") {
    $extractDir = Join-Path $tempDir "extract"
    Expand-Archive -LiteralPath $asset.Path -DestinationPath $extractDir -Force
    $binary = Get-ChildItem -Path $extractDir -Recurse -File |
      Where-Object { $_.Name.ToLowerInvariant().StartsWith("mihomo") -and $_.Extension.ToLowerInvariant() -eq ".exe" } |
      Select-Object -First 1
    if (-not $binary) {
      throw "Downloaded archive does not contain mihomo.exe."
    }
    Copy-Item -LiteralPath $binary.FullName -Destination $targetBinary -Force
    return
  }

  $source = [System.IO.File]::OpenRead($asset.Path)
  try {
    $gzip = [System.IO.Compression.GzipStream]::new($source, [System.IO.Compression.CompressionMode]::Decompress)
    try {
      $target = [System.IO.File]::Create($targetBinary)
      try {
        $gzip.CopyTo($target)
      } finally {
        $target.Dispose()
      }
    } finally {
      $gzip.Dispose()
    }
  } finally {
    $source.Dispose()
  }

  if (-not [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)) {
    chmod +x $targetBinary
  }
}

try {
  $platform = Get-CurrentPlatform
  $targetBinary = Join-Path $targetDir $platform.BinaryName

  New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
  New-Item -ItemType Directory -Force -Path $tempDir | Out-Null

  $tag = Resolve-LatestTag
  $asset = Download-FirstAvailableAsset $tag (Get-AssetNames $tag $platform) $tempDir
  Expand-MihomoArchive $asset $targetBinary $platform

  Write-Host "Downloaded $tag ($($asset.Name)) to $targetBinary"
} finally {
  if (Test-Path -LiteralPath $tempDir) {
    Remove-Item -LiteralPath $tempDir -Recurse -Force
  }
}
