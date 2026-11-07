# Build and run the capture self-test of the real recorder (platform/windows), or
# re-summarize an earlier log. Prints failed checks, the pass count and key numbers.
# -Spike runs the old M0 spike self-test instead (reference only).
param(
    [switch]$BuildOnly,
    [string]$LogFile,
    [switch]$Spike
)
$env:Path = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [Environment]::GetEnvironmentVariable("Path","User")

function Show-Summary([string[]]$lines) {
    $fail = @($lines | Where-Object { $_ -match '^FAIL ' })
    $lines | Where-Object { $_ -match '^SUMMARY ' }
    $fail
    $lines | Where-Object { $_ -match '^info ' } | ForEach-Object { $_.Trim() }
    return
}

if ($LogFile) {
    Show-Summary (Get-Content $LogFile)
    exit 0
}

$root = (git rev-parse --show-toplevel)
Set-Location $root

if ($Spike) {
    cargo build -p win-capture --release --message-format short 2>&1 | Where-Object { "$_" -match ': error|^error' } | Select-Object -First 30
    if ($LASTEXITCODE -ne 0) { "RESULT: BUILD FAILED"; exit 1 }
    $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
    & "$root\target\release\win-capture.exe" selftest --out "recordings\selftest-$stamp"
    exit $LASTEXITCODE
}

$build = cargo build --release -p babelsteps-windows --features selftest --bin capture-selftest --message-format short 2>&1 | ForEach-Object { "$_" }
if ($LASTEXITCODE -ne 0) {
    $build | Where-Object { $_ -match ': error|^error' } | Select-Object -First 30
    "RESULT: BUILD FAILED"
    exit 1
}
"Build OK."
if ($BuildOnly) { exit 0 }

$out = "$root\target\capture-selftest"
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$log = Join-Path $env:TEMP "babelsteps-capture-selftest-$stamp.log"
$lines = & "$root\target\release\capture-selftest.exe" $out 2>&1 | ForEach-Object { "$_" }
$code = $LASTEXITCODE
$lines | Set-Content -Encoding utf8 $log
Show-Summary $lines
"Full log: $log"
"Test-window crop (safe to view): $out\test-window-crop.png"
if ($code -eq 0) { "RESULT: PASS" } else { "RESULT: FAIL (exit $code)" }
exit $code
