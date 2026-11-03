# Build and run the capture self-test, or re-summarize an earlier log. Prints a short summary.
param(
    [switch]$BuildOnly,
    [string]$LogFile
)
$env:Path = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [Environment]::GetEnvironmentVariable("Path","User")

function Show-Summary([string[]]$lines) {
    $pass = @($lines | Where-Object { $_ -match '^\s*\[PASS\]' })
    $fail = @($lines | Where-Object { $_ -match '^\s*\[FAIL\]' })
    "Checks: $($pass.Count) passed, $($fail.Count) failed."
    $fail
    $lines | Where-Object {
        $_ -match '^Monitors:|click to pixels|hook callback|frame history|GDI on demand|CPU,|Memory MB|WGC frames|Output:|Report:'
    } | ForEach-Object { $_.Trim() }
    return
}

if ($LogFile) {
    Show-Summary (Get-Content $LogFile)
    exit 0
}

$root = (git rev-parse --show-toplevel)
Set-Location $root

$build = cargo build -p win-capture --release --message-format short 2>&1 | ForEach-Object { "$_" }
if ($LASTEXITCODE -ne 0) {
    $build | Where-Object { $_ -match ': error|^error' } | Select-Object -First 30
    "RESULT: BUILD FAILED"
    exit 1
}
"Build OK."
if ($BuildOnly) { exit 0 }

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$out = "recordings\selftest-$stamp"
$log = Join-Path $env:TEMP "babelsteps-selftest-$stamp.log"
$lines = & "$root\target\release\win-capture.exe" selftest --out $out 2>&1 | ForEach-Object { "$_" }
$code = $LASTEXITCODE
$lines | Set-Content -Encoding utf8 $log
Show-Summary $lines
"Full log: $log"
if ($code -eq 0) { "RESULT: PASS" } else { "RESULT: FAIL (exit $code)" }
exit $code
