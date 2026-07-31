param(
  [Parameter(Mandatory = $true)]
  [ValidateSet("linux-amd64", "linux-arm64", "linux-x86_64", "linux-aarch64", "windows-amd64", "windows-x86_64", "macos-arm64", "macos-aarch64", "macos-x86_64")]
  [string]$Target,
  [string]$Version = "latest"
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot
if ($IsWindows -and $Target -like "linux-*") {
  throw "Linux archives must be packaged from Linux or WSL so tar preserves executable modes. Use scripts/package-local.sh there."
}

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

function New-LinuxArchive {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Artifact,
    [Parameter(Mandatory = $true)]
    [string]$RustTarget
  )

  $distRoot = Join-Path $repoRoot "dist"
  $releaseDir = Join-Path $distRoot $Artifact
  if (Test-Path -LiteralPath $releaseDir) {
    Remove-Item -LiteralPath $releaseDir -Recurse -Force
  }
  New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null

  $binaryPath = Join-Path $repoRoot "target/$RustTarget/release/rweb-clash-bin"
  if (-not (Test-Path -LiteralPath $binaryPath -PathType Leaf)) {
    throw "release binary not found: $binaryPath"
  }

  Copy-Item -LiteralPath $binaryPath -Destination (Join-Path $releaseDir "rweb-clash") -Force
  Copy-Item -LiteralPath $binaryPath -Destination (Join-Path $distRoot "$Artifact.bin") -Force
  Copy-Item -LiteralPath (Join-Path $repoRoot "README.md") -Destination (Join-Path $releaseDir "README.md") -Force
  Copy-Item -LiteralPath (Join-Path $repoRoot "LICENSE") -Destination (Join-Path $releaseDir "LICENSE") -Force
  Copy-Item -LiteralPath (Join-Path $repoRoot "packaging/linux/README.md") -Destination (Join-Path $releaseDir "LINUX.md") -Force
  Copy-Item -LiteralPath (Join-Path $repoRoot "packaging/linux/install.sh") -Destination $releaseDir -Force
  Copy-Item -LiteralPath (Join-Path $repoRoot "packaging/linux/install-systemd.sh") -Destination $releaseDir -Force
  Copy-Item -LiteralPath (Join-Path $repoRoot "packaging/linux/rweb-clash.service") -Destination $releaseDir -Force
  Copy-Item -LiteralPath (Join-Path $repoRoot "packaging/linux/rweb-clash-system.service") -Destination $releaseDir -Force
  Copy-Item -LiteralPath (Join-Path $repoRoot "packaging/linux/rweb-clash-ready.service") -Destination $releaseDir -Force
  Copy-Item -LiteralPath (Join-Path $repoRoot "packaging/linux/rweb-clash.env") -Destination $releaseDir -Force
  Copy-Item -LiteralPath (Join-Path $repoRoot "scripts/release-smoke.sh") -Destination $releaseDir -Force
  Copy-Item -LiteralPath (Join-Path $repoRoot "scripts/release-smoke.ps1") -Destination $releaseDir -Force

  if (-not $IsWindows) {
    Invoke-Native {
      chmod +x `
        (Join-Path $releaseDir "rweb-clash") `
        (Join-Path $distRoot "$Artifact.bin") `
        (Join-Path $releaseDir "install.sh") `
        (Join-Path $releaseDir "install-systemd.sh") `
        (Join-Path $releaseDir "release-smoke.sh")
    }
  }

  $archive = Join-Path $distRoot "$Artifact.tar.gz"
  Remove-Item -LiteralPath $archive -Force -ErrorAction SilentlyContinue
  tar -C $distRoot -czf $archive $Artifact

  $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()
  "$hash  $Artifact.tar.gz" | Set-Content -LiteralPath "$archive.sha256" -Encoding ascii
  $binaryArtifact = Join-Path $distRoot "$Artifact.bin"
  $binaryHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $binaryArtifact).Hash.ToLowerInvariant()
  $binaryChecksum = Join-Path $distRoot "$Artifact.bin.sha256"
  "$binaryHash  $Artifact.bin" | Set-Content -LiteralPath $binaryChecksum -Encoding ascii

  if (-not $IsWindows) {
    & bash (Join-Path $repoRoot "scripts/verify-linux-archive.sh") --archive $archive
  } else {
    Write-Host "Created Linux release archive: $archive"
    Write-Host "Run scripts/verify-linux-archive.sh on Linux to verify executable bits."
  }
}

& (Join-Path $PSScriptRoot "package-core.ps1") -Target $Target -Version $Version
& (Join-Path $PSScriptRoot "package-rule-sets.ps1")
& (Join-Path $PSScriptRoot "package-runtime-assets.ps1")
Invoke-Native { pnpm --dir (Join-Path $repoRoot "web") build }

switch ($Target) {
  { $_ -in @("linux-amd64", "linux-x86_64") } {
    $rustTarget = "x86_64-unknown-linux-musl"
    Invoke-Native { cross build -p rweb-clash-bin --features embedded-assets --release --locked --target $rustTarget }
    New-LinuxArchive -Artifact "rweb-clash-linux-amd64" -RustTarget $rustTarget
    break
  }
  { $_ -in @("linux-arm64", "linux-aarch64") } {
    $rustTarget = "aarch64-unknown-linux-musl"
    Invoke-Native { cross build -p rweb-clash-bin --features embedded-assets --release --locked --target $rustTarget }
    New-LinuxArchive -Artifact "rweb-clash-linux-arm64" -RustTarget $rustTarget
    break
  }
  { $_ -in @("windows-amd64", "windows-x86_64", "macos-arm64", "macos-aarch64", "macos-x86_64") } {
    & (Join-Path $PSScriptRoot "prepare-tauri-resources.ps1") -Target $Target
    & (Join-Path $PSScriptRoot "verify-tauri-resources.ps1") -Target $Target
    if ($Target -like "macos-*") {
      Invoke-Native { pnpm --dir (Join-Path $repoRoot "apps/desktop") tauri build --config src-tauri/tauri.macos.conf.json }
    } else {
      Invoke-Native { pnpm --dir (Join-Path $repoRoot "apps/desktop") tauri build }
    }
    break
  }
}
