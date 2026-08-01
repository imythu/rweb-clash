[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [string]$Version,
  [string]$MacosBundleVersion,
  [string]$WindowsBundleVersion,
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
if ($MacosBundleVersion -and $MacosBundleVersion -notmatch '^[0-9]{12}$') {
  throw "macOS bundle version must be a 12-digit YYYYMMDDHHmm value: $MacosBundleVersion"
}
if ($WindowsBundleVersion) {
  $windowsParts = @($WindowsBundleVersion -split '\.')
  $invalidWindowsParts = @($windowsParts | Where-Object { $_ -notmatch '^(0|[1-9][0-9]*)$' })
  if ($windowsParts.Count -ne 4 -or $invalidWindowsParts.Count -gt 0) {
    throw "Windows bundle version must contain four numeric components: $WindowsBundleVersion"
  }
  $windowsNumbers = @($windowsParts | ForEach-Object { [uint64]$_ })
  if ($windowsNumbers[0] -gt 255 -or $windowsNumbers[1] -gt 255 -or
      $windowsNumbers[2] -gt 65535 -or $windowsNumbers[3] -gt 65535) {
    throw "Windows bundle version exceeds MSI component limits: $WindowsBundleVersion"
  }
}

$repoRoot = if ([string]::IsNullOrWhiteSpace($Root)) {
  Resolve-Path (Join-Path $PSScriptRoot "..")
} else {
  Resolve-Path $Root
}

$workspaceCargoPath = Join-Path $repoRoot "Cargo.toml"
$workspaceCargoLockPath = Join-Path $repoRoot "Cargo.lock"
$desktopCargoPath = Join-Path $repoRoot "apps/desktop/src-tauri/Cargo.toml"
$desktopCargoLockPath = Join-Path $repoRoot "apps/desktop/src-tauri/Cargo.lock"
$tauriConfigPath = Join-Path $repoRoot "apps/desktop/src-tauri/tauri.conf.json"
$desktopPackagePath = Join-Path $repoRoot "apps/desktop/package.json"
$requiredFiles = @(
  $workspaceCargoPath,
  $workspaceCargoLockPath,
  $desktopCargoPath,
  $desktopCargoLockPath,
  $tauriConfigPath,
  $desktopPackagePath
)
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

function Set-TopLevelJsonVersion {
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
  $versionPattern = [regex]::new('(?m)^(\s{2}"version"\s*:\s*)"[^"]*"')
  $versionMatches = $versionPattern.Matches($Content)
  if ($versionMatches.Count -ne 1) {
    throw "Expected exactly one formatted top-level JSON version property, found $($versionMatches.Count)."
  }
  $match = $versionMatches[0]
  $replacement = $match.Groups[1].Value + '"' + $ReleaseVersion + '"'
  return $Content.Substring(0, $match.Index) + $replacement + $Content.Substring($match.Index + $match.Length)
}

function Set-JsonVersion {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Content,
    [Parameter(Mandatory = $true)]
    [string]$ReleaseVersion
  )

  return Set-TopLevelJsonVersion -Content $Content -ReleaseVersion $ReleaseVersion
}

function Set-CargoLockPackageVersions {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Content,
    [Parameter(Mandatory = $true)]
    [string[]]$PackageNames,
    [Parameter(Mandatory = $true)]
    [string]$ReleaseVersion
  )

  $updates = @{}
  foreach ($packageName in $PackageNames) {
    $updates[$packageName] = 0
  }

  $parts = [regex]::Split($Content, '(\r\n|\n|\r)')
  $inPackage = $false
  $currentPackage = $null
  for ($index = 0; $index -lt $parts.Length; $index += 2) {
    $line = $parts[$index]
    if ($line -eq '[[package]]') {
      $inPackage = $true
      $currentPackage = $null
      continue
    }
    if ($line -match '^\[.*\]$') {
      $inPackage = $false
      $currentPackage = $null
      continue
    }
    if (-not $inPackage) {
      continue
    }
    if ($line -match '^\s*name\s*=\s*"([^"]+)"\s*$') {
      $currentPackage = $matches[1]
      continue
    }
    if ($currentPackage -and $updates.ContainsKey($currentPackage) -and
        $line -match '^(\s*version\s*=\s*)"[^"]*"(\s*(?:#.*)?)$') {
      $parts[$index] = "$($matches[1])`"$ReleaseVersion`"$($matches[2])"
      $updates[$currentPackage]++
    }
  }

  foreach ($packageName in $PackageNames) {
    if ($updates[$packageName] -ne 1) {
      throw "Expected exactly one $packageName package version in Cargo.lock, found $($updates[$packageName])."
    }
  }
  return ($parts -join '')
}

function Set-TauriJsonVersion {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Content,
    [Parameter(Mandatory = $true)]
    [string]$ReleaseVersion,
    [string]$MacBundleVersion,
    [string]$WindowsInstallerVersion
  )

  $document = $Content | ConvertFrom-Json
  if ($null -eq $document.PSObject.Properties['version'] -or
      $null -eq $document.PSObject.Properties['bundle']) {
    throw "Tauri JSON must contain top-level version and bundle properties."
  }
  if (-not $MacBundleVersion -and -not $WindowsInstallerVersion) {
    return Set-TopLevelJsonVersion -Content $Content -ReleaseVersion $ReleaseVersion
  }
  $document.version = $ReleaseVersion

  if ($MacBundleVersion) {
    if ($null -eq $document.bundle.PSObject.Properties['macOS']) {
      $document.bundle | Add-Member -MemberType NoteProperty -Name 'macOS' -Value ([pscustomobject]@{})
    }
    if ($null -eq $document.bundle.macOS.PSObject.Properties['bundleVersion']) {
      $document.bundle.macOS | Add-Member -MemberType NoteProperty -Name 'bundleVersion' -Value $MacBundleVersion
    } else {
      $document.bundle.macOS.bundleVersion = $MacBundleVersion
    }
  }

  if ($WindowsInstallerVersion) {
    if ($null -eq $document.bundle.PSObject.Properties['windows']) {
      $document.bundle | Add-Member -MemberType NoteProperty -Name 'windows' -Value ([pscustomobject]@{})
    }
    if ($null -eq $document.bundle.windows.PSObject.Properties['wix']) {
      $document.bundle.windows | Add-Member -MemberType NoteProperty -Name 'wix' -Value ([pscustomobject]@{})
    }
    if ($null -eq $document.bundle.windows.wix.PSObject.Properties['version']) {
      $document.bundle.windows.wix | Add-Member -MemberType NoteProperty -Name 'version' -Value $WindowsInstallerVersion
    } else {
      $document.bundle.windows.wix.version = $WindowsInstallerVersion
    }
  }

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
$workspaceCargoLock = Set-CargoLockPackageVersions `
  -Content ([System.IO.File]::ReadAllText($workspaceCargoLockPath)) `
  -PackageNames @("rweb-clash", "rweb-clash-bin", "rweb-clash-macos-helper", "rweb-clash-windows-helper") `
  -ReleaseVersion $candidate
$desktopCargoLock = Set-CargoLockPackageVersions `
  -Content ([System.IO.File]::ReadAllText($desktopCargoLockPath)) `
  -PackageNames @("rweb-clash", "rweb-clash-desktop") `
  -ReleaseVersion $candidate
$tauriConfig = Set-TauriJsonVersion `
  -Content ([System.IO.File]::ReadAllText($tauriConfigPath)) `
  -ReleaseVersion $candidate `
  -MacBundleVersion $MacosBundleVersion `
  -WindowsInstallerVersion $WindowsBundleVersion
$desktopPackage = Set-JsonVersion `
  -Content ([System.IO.File]::ReadAllText($desktopPackagePath)) `
  -ReleaseVersion $candidate

$utf8NoBom = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText($workspaceCargoPath, $workspaceCargo, $utf8NoBom)
[System.IO.File]::WriteAllText($workspaceCargoLockPath, $workspaceCargoLock, $utf8NoBom)
[System.IO.File]::WriteAllText($desktopCargoPath, $desktopCargo, $utf8NoBom)
[System.IO.File]::WriteAllText($desktopCargoLockPath, $desktopCargoLock, $utf8NoBom)
[System.IO.File]::WriteAllText($tauriConfigPath, $tauriConfig, $utf8NoBom)
[System.IO.File]::WriteAllText($desktopPackagePath, $desktopPackage, $utf8NoBom)

Write-Host "Synchronized release version $candidate across Cargo and desktop manifests."
