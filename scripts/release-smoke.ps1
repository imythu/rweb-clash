param(
  [string]$Binary = "target/debug/rweb-clash-bin.exe",
  [string]$Listen = "127.0.0.1:32990",
  [int]$MixedPort = 32991,
  [switch]$VerifyEmbeddedAssets
)

$ErrorActionPreference = "Stop"
$runId = [guid]::NewGuid().ToString("N")
$root = Join-Path $PWD ".tmp-smoke-runtime-$runId"
$stdout = Join-Path $PWD ".tmp-smoke-$runId.out.log"
$stderr = Join-Path $PWD ".tmp-smoke-$runId.err.log"

function Write-SmokeDiagnostics {
  param([System.Management.Automation.ErrorRecord]$LastError)
  if ($LastError) {
    Write-Host "--- last error ---"
    Write-Host $LastError.ToString()
  }
  Write-Host "--- stdout ---"
  if (Test-Path -LiteralPath $stdout) { Get-Content -LiteralPath $stdout }
  Write-Host "--- stderr ---"
  if (Test-Path -LiteralPath $stderr) { Get-Content -LiteralPath $stderr }
}

if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
Remove-Item -LiteralPath $stdout, $stderr -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $root | Out-Null

$process = Start-Process -FilePath $Binary `
  -ArgumentList @("--listen", $Listen, "--data-dir", $root, "--log-level", "warn") `
  -WorkingDirectory $PWD `
  -NoNewWindow `
  -PassThru `
  -RedirectStandardOutput $stdout `
  -RedirectStandardError $stderr

try {
  $ready = $false
  $lastError = $null
  $setup = $null
  for ($i = 0; $i -lt 40; $i++) {
    try {
      $setup = Invoke-RestMethod -Uri "http://$Listen/api/setup/status" -TimeoutSec 2
      $diagnostics = Invoke-RestMethod -Uri "http://$Listen/api/diagnostics/export" -TimeoutSec 2
      if (-not $diagnostics.StartsWith("# rweb-clash diagnostics")) {
        throw "Diagnostics export header mismatch."
      }
      $ready = $true
      break
    } catch {
      $lastError = $_
      Start-Sleep -Milliseconds 500
    }
  }
  if (-not $ready) {
    Write-SmokeDiagnostics -LastError $lastError
    throw "rweb-clash did not become ready on $Listen"
  }

  if ($VerifyEmbeddedAssets) {
    try {
      $coreName = if ($env:OS -eq "Windows_NT") { "mihomo.exe" } else { "mihomo" }
      $corePath = Join-Path $root "cache-core/$coreName"
      $geoipPath = Join-Path $root "data/profiles/geoip.metadb"
      $ruleSetDir = Join-Path $root "data/profiles/rule-sets"
      foreach ($asset in @($corePath, $geoipPath)) {
        if (-not (Test-Path -LiteralPath $asset -PathType Leaf) -or (Get-Item -LiteralPath $asset).Length -le 0) {
          throw "Embedded runtime asset was not restored: $asset"
        }
      }
      $ruleSets = @(Get-ChildItem -LiteralPath $ruleSetDir -File -Filter "*.list")
      if ($ruleSets.Count -ne 13) {
        throw "Expected 13 embedded rule sets, found $($ruleSets.Count)."
      }
      Invoke-RestMethod -Method Patch -Uri "http://$Listen/api/configs" `
        -ContentType "application/json" `
        -Body (@{ mixed_port = $MixedPort } | ConvertTo-Json -Compress) | Out-Null
      $coreStatus = Invoke-RestMethod -Method Post -Uri "http://$Listen/api/core/start" -TimeoutSec 150
      if ($coreStatus.state -ne "running") {
        throw "Mihomo did not reach the running state: $($coreStatus | ConvertTo-Json -Compress)"
      }
      Invoke-RestMethod -Method Post -Uri "http://$Listen/api/core/stop" -TimeoutSec 10 | Out-Null
    } catch {
      Write-SmokeDiagnostics -LastError $_
      throw
    }
  }

  [pscustomobject]@{
    ok = $true
    needsOnboarding = $setup.needsOnboarding
    coreReady = $setup.coreReady
    subscriptions = $setup.subscriptionCount
  } | ConvertTo-Json -Depth 4
} finally {
  Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 300
  Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath $stdout, $stderr -Force -ErrorAction SilentlyContinue
}
