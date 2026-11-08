# Build and test the workspace. Prints only what matters; the full log goes to a file.
$env:Path = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [Environment]::GetEnvironmentVariable("Path","User")
Set-Location (git rev-parse --show-toplevel)

$log = Join-Path $env:TEMP "babelsteps-check.log"
$output = cargo test --workspace --message-format short 2>&1 | ForEach-Object { "$_" }
$code = $LASTEXITCODE
$output | Set-Content -Encoding utf8 $log

$errors = @($output | Where-Object { $_ -match ': error|^error' })
$warnings = @($output | Where-Object { $_ -match ': warning:' })
$failed = @($output | Where-Object { $_ -match '^test .* FAILED$' })
$results = @($output | Where-Object { $_ -match '^test result:' })
$passed = 0
foreach ($r in $results) { if ($r -match '(\d+) passed') { $passed += [int]$Matches[1] } }

$errors | Select-Object -First 30
$failed | Select-Object -First 30
"Warnings: $($warnings.Count). Tests passed: $passed. Tests failed: $($failed.Count)."
$net = powershell -NoProfile -File (Join-Path $PSScriptRoot "no-network.ps1") 2>&1 | ForEach-Object { "$_" }
$netCode = $LASTEXITCODE
$net
if ($netCode -ne 0 -and $code -eq 0) { $code = 1 }
"Full log: $log"
if ($code -eq 0) { "RESULT: PASS" } else { "RESULT: FAIL (cargo exit $code)" }
exit $code
