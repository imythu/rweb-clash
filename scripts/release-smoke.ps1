param(
  [string]$Binary = "target/debug/rweb-clash-bin.exe",
  [string]$Listen = "127.0.0.1:32990"
)

$ErrorActionPreference = "Stop"
$root = Join-Path $PWD ".tmp-smoke-runtime"
$stdout = Join-Path $PWD ".tmp-smoke.out.log"
$stderr = Join-Path $PWD ".tmp-smoke.err.log"

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
  for ($i = 0; $i -lt 40; $i++) {
    try {
      $setup = Invoke-RestMethod -Uri "http://$Listen/api/setup/status" -TimeoutSec 2
      $diagnostics = Invoke-RestMethod -Uri "http://$Listen/api/diagnostics/export" -TimeoutSec 2
      if (-not $diagnostics.StartsWith("# rweb-clash diagnostics")) {
        throw "Diagnostics export header mismatch."
      }
      $ready = $true
      [pscustomobject]@{
        ok = $true
        needsOnboarding = $setup.needsOnboarding
        coreReady = $setup.coreReady
        subscriptions = $setup.subscriptionCount
      } | ConvertTo-Json -Depth 4
      break
    } catch {
      Start-Sleep -Milliseconds 500
    }
  }
  if (-not $ready) {
    throw "rweb-clash did not become ready on $Listen"
  }
} finally {
  Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 300
  Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath $stdout, $stderr -Force -ErrorAction SilentlyContinue
}
