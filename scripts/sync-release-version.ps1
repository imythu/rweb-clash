[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [string]$Version,
  [string]$Root
)

$ErrorActionPreference = "Stop"

$candidate = $Version.Trim()
if ($candidate.StartsWith("v", [System.StringComparison]::Ordinal)) {
  $candidate = $candidate.Substring(1)
}

$numericIdentifier = '(?:0|[1-9][0-9]*)'
$nonNumericIdentifier = '(?:[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
$preReleaseIdentifier = "(?:$numericIdentifier|$nonNumericIdentifier)"
$semVerPattern = "^$numericIdentifier\.$numericIdentifier\.$numericIdentifier(?:-$preReleaseIdentifier(?:\.$preReleaseIdentifier)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
if ($candidate -notmatch $semVerPattern) {
  throw "Release version must be SemVer with an optional lowercase v prefix: $Version"
}

$repoRoot = if ([string]::IsNullOrWhiteSpace($Root)) {
  Resolve-Path (Join-Path $PSScriptRoot "..")
} else {
  Resolve-Path $Root
}

$workspaceCargoPath = Join-Path $repoRoot "Cargo.toml"
$desktopCargoPath = Join-Path $repoRoot "apps/desktop/src-tauri/Cargo.toml"
$tauriConfigPath = Join-Path $repoRoot "apps/desktop/src-tauri/tauri.conf.json"
$desktopPackagePath = Join-Path $repoRoot "apps/desktop/package.json"
$requiredFiles = @($workspaceCargoPath, $desktopCargoPath, $tauriConfigPath, $desktopPackagePath)
foreach ($path in $requiredFiles) {
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
    throw "Release version target not found: $path"
  }
}

function Set-TomlSectionVersion {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Content,
    [Parameter(Mandatory = $true)]
    [string]$Section,
    [Parameter(Mandatory = $true)]
    [string]$ReleaseVersion
  )

  $parts = [regex]::Split($Content, '(\r\n|\n|\r)')
  $currentSection = $null
  $updated = 0
  for ($index = 0; $index -lt $parts.Length; $index += 2) {
    $line = $parts[$index]
    if ($line -match '^\s*\[([^]]+)\]\s*(?:#.*)?$') {
      $currentSection = $matches[1]
      continue
    }
    if ($currentSection -eq $Section -and $line -match '^(\s*version\s*=\s*)"[^"]*"(\s*(?:#.*)?)$') {
      $parts[$index] = "$($matches[1])`"$ReleaseVersion`"$($matches[2])"
      $updated++
    }
  }

  if ($updated -ne 1) {
    throw "Expected exactly one version in TOML section [$Section], found $updated."
  }
  return ($parts -join '')
}

function Set-JsonVersion {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Content,
    [Parameter(Mandatory = $true)]
    [string]$ReleaseVersion
  )

  $document = $Content | ConvertFrom-Json
  if ($null -eq $document.PSObject.Properties['version']) {
    throw "JSON document does not contain a top-level version property."
  }
  $document.version = $ReleaseVersion
  return (($document | ConvertTo-Json -Depth 100) + [Environment]::NewLine)
}

$workspaceCargo = Set-TomlSectionVersion `
  -Content ([System.IO.File]::ReadAllText($workspaceCargoPath)) `
  -Section "workspace.package" `
  -ReleaseVersion $candidate
$desktopCargo = Set-TomlSectionVersion `
  -Content ([System.IO.File]::ReadAllText($desktopCargoPath)) `
  -Section "package" `
  -ReleaseVersion $candidate
$tauriConfig = Set-JsonVersion `
  -Content ([System.IO.File]::ReadAllText($tauriConfigPath)) `
  -ReleaseVersion $candidate
$desktopPackage = Set-JsonVersion `
  -Content ([System.IO.File]::ReadAllText($desktopPackagePath)) `
  -ReleaseVersion $candidate

$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText($workspaceCargoPath, $workspaceCargo, $utf8NoBom)
[System.IO.File]::WriteAllText($desktopCargoPath, $desktopCargo, $utf8NoBom)
[System.IO.File]::WriteAllText($tauriConfigPath, $tauriConfig, $utf8NoBom)
[System.IO.File]::WriteAllText($desktopPackagePath, $desktopPackage, $utf8NoBom)

Write-Host "Synchronized release version $candidate across Cargo and desktop manifests."
