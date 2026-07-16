param()

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$tauriDir = Join-Path $repoRoot "apps/desktop/src-tauri"
$windowsConfig = Join-Path $tauriDir "tauri.windows.conf.json"

function Write-Utf8NoBom {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Path,
    [Parameter(Mandatory = $true)]
    [string]$Content
  )
  [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

$thumbprint = $env:WINDOWS_CERTIFICATE_THUMBPRINT
$digestAlgorithm = if ($env:WINDOWS_CERTIFICATE_DIGEST_ALGORITHM) { $env:WINDOWS_CERTIFICATE_DIGEST_ALGORITHM } else { "sha256" }
$timestampUrl = $env:WINDOWS_CERTIFICATE_TIMESTAMP_URL

if ($env:WINDOWS_CERTIFICATE -and $env:WINDOWS_CERTIFICATE_PASSWORD) {
  $tempRoot = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [System.IO.Path]::GetTempPath() }
  $certificatePath = Join-Path $tempRoot "rweb-clash-windows-code-signing.pfx"
  [IO.File]::WriteAllBytes($certificatePath, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
  $securePassword = ConvertTo-SecureString -String $env:WINDOWS_CERTIFICATE_PASSWORD -Force -AsPlainText
  $certificate = Import-PfxCertificate -FilePath $certificatePath -CertStoreLocation Cert:\CurrentUser\My -Password $securePassword
  if (-not $thumbprint) {
    $thumbprint = $certificate.Thumbprint
  }
}

if (-not $thumbprint) {
  Write-Host "No Windows code signing certificate configured; building unsigned Windows packages."
  Write-Utf8NoBom -Path $windowsConfig -Content "{}"
  exit 0
}

if (-not $timestampUrl) {
  throw "WINDOWS_CERTIFICATE_TIMESTAMP_URL is required when Windows signing is enabled."
}

$config = [ordered]@{
  bundle = [ordered]@{
    windows = [ordered]@{
      certificateThumbprint = $thumbprint
      digestAlgorithm = $digestAlgorithm
      timestampUrl = $timestampUrl
    }
  }
}

$json = $config | ConvertTo-Json -Depth 8
Write-Utf8NoBom -Path $windowsConfig -Content $json
Write-Host "Configured Windows code signing with certificate thumbprint $thumbprint."
