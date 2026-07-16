$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$manifestPath = Join-Path $repoRoot "packaging/manifests/rule-sets.toml"
$targetDir = Join-Path $repoRoot "packaging/cache/rule-sets"
$manifestOut = Join-Path $targetDir "manifest.json"

New-Item -ItemType Directory -Force -Path $targetDir | Out-Null

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

$outputs = @()
foreach ($item in $items) {
  $fileName = "$($item.id).list"
  $target = Join-Path $targetDir $fileName
  Invoke-WebRequest -Uri $item.url -Headers @{ "User-Agent" = "rweb-clash" } -OutFile $target
  $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $target).Hash.ToLowerInvariant()
  $outputs += [ordered]@{
    id = $item.id
    name = $item.name
    url = $item.url
    file = $fileName
    sha256 = $hash
    bytes = (Get-Item -LiteralPath $target).Length
  }
  Write-Host "Downloaded $($item.name) to $target"
}

@{
  generatedAt = [DateTimeOffset]::UtcNow.ToString("o")
  ruleSets = $outputs
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $manifestOut -Encoding utf8

Write-Host "Wrote $manifestOut"
