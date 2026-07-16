$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

function Invoke-Step([string]$Name, [scriptblock]$Command) {
  Write-Host ""
  Write-Host "==> $Name"
  & $Command
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

Invoke-Step "Check PowerShell script syntax" {
  $scripts = @(
    "scripts/package-core.ps1",
    "scripts/package-rule-sets.ps1",
    "scripts/package-runtime-assets.ps1",
    "scripts/package-local.ps1",
    "scripts/prepare-tauri-resources.ps1",
    "scripts/verify-tauri-resources.ps1",
    "scripts/release-smoke.ps1",
    "scripts/verify-release-local.ps1",
    "scripts/verify-release-workflow.ps1",
    "scripts/configure-tauri-signing.ps1",
    "scripts/sync-release-version.ps1"
  )
  foreach ($script in $scripts) {
    $tokens = $null
    $errors = $null
    [System.Management.Automation.Language.Parser]::ParseFile($script, [ref]$tokens, [ref]$errors) | Out-Null
    if ($errors.Count -gt 0) {
      $errors | ForEach-Object { Write-Error "$script`: $($_.Message)" }
      throw "PowerShell syntax check failed for $script"
    }
  }
}

Invoke-Step "Run Rust tests" {
  Invoke-Native { cargo test }
}

Invoke-Step "Check release workflow structure" {
  Invoke-Native { powershell -ExecutionPolicy Bypass -File scripts/verify-release-workflow.ps1 }
}

Invoke-Step "Check release version synchronization" {
  $tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("rweb-clash-version-test-" + [guid]::NewGuid().ToString("N"))
  try {
    New-Item -ItemType Directory -Force -Path (Join-Path $tempRoot "apps/desktop/src-tauri") | Out-Null
    Copy-Item -LiteralPath (Join-Path $repoRoot "Cargo.toml") -Destination (Join-Path $tempRoot "Cargo.toml")
    Copy-Item -LiteralPath (Join-Path $repoRoot "apps/desktop/src-tauri/Cargo.toml") -Destination (Join-Path $tempRoot "apps/desktop/src-tauri/Cargo.toml")
    Copy-Item -LiteralPath (Join-Path $repoRoot "apps/desktop/src-tauri/tauri.conf.json") -Destination (Join-Path $tempRoot "apps/desktop/src-tauri/tauri.conf.json")
    Copy-Item -LiteralPath (Join-Path $repoRoot "apps/desktop/package.json") -Destination (Join-Path $tempRoot "apps/desktop/package.json")

    Invoke-Native {
      powershell -NoProfile -ExecutionPolicy Bypass -File scripts/sync-release-version.ps1 `
        -Version "2026.717.234" `
        -MacosBundleVersion "202607170234" `
        -WindowsBundleVersion "26.7.17.234" `
        -Root $tempRoot
    }

    $expectedVersion = "2026.717.234"
    $workspaceCargo = Get-Content -LiteralPath (Join-Path $tempRoot "Cargo.toml") -Raw
    $desktopCargo = Get-Content -LiteralPath (Join-Path $tempRoot "apps/desktop/src-tauri/Cargo.toml") -Raw
    $tauriConfig = Get-Content -LiteralPath (Join-Path $tempRoot "apps/desktop/src-tauri/tauri.conf.json") -Raw | ConvertFrom-Json
    $desktopPackage = Get-Content -LiteralPath (Join-Path $tempRoot "apps/desktop/package.json") -Raw | ConvertFrom-Json
    if ($workspaceCargo -notmatch "(?m)^version = `"$([regex]::Escape($expectedVersion))`"$") {
      throw "Workspace Cargo version was not synchronized."
    }
    if ($desktopCargo -notmatch "(?m)^version = `"$([regex]::Escape($expectedVersion))`"$") {
      throw "Desktop Cargo version was not synchronized."
    }
    if ($tauriConfig.version -ne $expectedVersion -or $desktopPackage.version -ne $expectedVersion) {
      throw "Desktop JSON versions were not synchronized."
    }
    if ($tauriConfig.bundle.macOS.bundleVersion -ne "202607170234" -or
        $tauriConfig.bundle.windows.wix.version -ne "26.7.17.234") {
      throw "Platform bundle versions were not synchronized."
    }

    $previousErrorPreference = $ErrorActionPreference
    try {
      $ErrorActionPreference = "SilentlyContinue"
      powershell -NoProfile -ExecutionPolicy Bypass -File scripts/sync-release-version.ps1 `
        -Version "v01.2.3" `
        -Root $tempRoot 2>$null
      $invalidExitCode = $LASTEXITCODE
    } finally {
      $ErrorActionPreference = $previousErrorPreference
    }
    if ($invalidExitCode -eq 0) {
      throw "Release version synchronization accepted invalid SemVer."
    }
  } finally {
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
  }
}

Invoke-Step "Check Tauri crate" {
  Invoke-Native { cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml }
}

Invoke-Step "Check Windows signing config generation" {
  $configPath = Join-Path $repoRoot "apps/desktop/src-tauri/tauri.windows.conf.json"
  $previousThumbprint = $env:WINDOWS_CERTIFICATE_THUMBPRINT
  $previousTimestampUrl = $env:WINDOWS_CERTIFICATE_TIMESTAMP_URL
  $previousDigest = $env:WINDOWS_CERTIFICATE_DIGEST_ALGORITHM
  try {
    $env:WINDOWS_CERTIFICATE_THUMBPRINT = "ABCDEF1234567890"
    $env:WINDOWS_CERTIFICATE_TIMESTAMP_URL = "http://timestamp.example.invalid"
    $env:WINDOWS_CERTIFICATE_DIGEST_ALGORITHM = "sha256"
    Invoke-Native { powershell -ExecutionPolicy Bypass -File scripts/configure-tauri-signing.ps1 }
    $bytes = [System.IO.File]::ReadAllBytes($configPath)
    if ($bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) {
      throw "Windows signing config must be UTF-8 without BOM."
    }
    $config = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
    if ($config.bundle.windows.certificateThumbprint -ne $env:WINDOWS_CERTIFICATE_THUMBPRINT) {
      throw "Windows signing config did not contain the expected certificate thumbprint."
    }
    if ($config.bundle.windows.timestampUrl -ne $env:WINDOWS_CERTIFICATE_TIMESTAMP_URL) {
      throw "Windows signing config did not contain the expected timestamp URL."
    }
  } finally {
    if ($null -eq $previousThumbprint) {
      Remove-Item Env:\WINDOWS_CERTIFICATE_THUMBPRINT -ErrorAction SilentlyContinue
    } else {
      $env:WINDOWS_CERTIFICATE_THUMBPRINT = $previousThumbprint
    }
    if ($null -eq $previousTimestampUrl) {
      Remove-Item Env:\WINDOWS_CERTIFICATE_TIMESTAMP_URL -ErrorAction SilentlyContinue
    } else {
      $env:WINDOWS_CERTIFICATE_TIMESTAMP_URL = $previousTimestampUrl
    }
    if ($null -eq $previousDigest) {
      Remove-Item Env:\WINDOWS_CERTIFICATE_DIGEST_ALGORITHM -ErrorAction SilentlyContinue
    } else {
      $env:WINDOWS_CERTIFICATE_DIGEST_ALGORITHM = $previousDigest
    }
    Remove-Item -LiteralPath $configPath -Force -ErrorAction SilentlyContinue
  }
}

Invoke-Step "Install web workspace with frozen lockfile" {
  $env:CI = "true"
  Invoke-Native { corepack pnpm --dir web install --frozen-lockfile }
}

Invoke-Step "Build Tauri web assets" {
  Invoke-Native { corepack pnpm --dir web build:tauri }
}

$verificationTarget = Join-Path $repoRoot ("target/release-verification-" + [guid]::NewGuid().ToString("N"))
$previousCargoTarget = $env:CARGO_TARGET_DIR
try {
  $env:CARGO_TARGET_DIR = $verificationTarget
  Invoke-Step "Build isolated embedded backend binary" {
    Invoke-Native { cargo build -p rweb-clash-bin --features embedded-assets }
  }

  Invoke-Step "Run embedded asset and Mihomo smoke" {
    $binaryName = if ($env:OS -eq "Windows_NT") { "rweb-clash-bin.exe" } else { "rweb-clash-bin" }
    $binary = Join-Path $verificationTarget "debug/$binaryName"
    Invoke-Native {
      powershell -ExecutionPolicy Bypass -File scripts/release-smoke.ps1 `
        -Binary $binary `
        -VerifyEmbeddedAssets
    }
  }
} finally {
  if ($null -eq $previousCargoTarget) {
    Remove-Item Env:\CARGO_TARGET_DIR -ErrorAction SilentlyContinue
  } else {
    $env:CARGO_TARGET_DIR = $previousCargoTarget
  }
  Remove-Item -LiteralPath $verificationTarget -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "Local release verification passed."
