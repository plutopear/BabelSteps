# Fails if the app could contain network code: a networking crate, a Tauri
# http/shell/opener/updater/websocket plugin, tokio's net feature, or a
# capability that grants one. Prints problems only. Run by check.ps1.
# Refresh PATH only when cargo is missing (a shell started before Rust was
# installed). CI runners already have it, and resetting PATH there would drop it.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $env:Path = [Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [Environment]::GetEnvironmentVariable("Path","User")
}
Set-Location (git rev-parse --show-toplevel)

$denyCrates = 'hyper|hyper-util|reqwest|mio|socket2|h2|ureq|curl|isahc|rustls|native-tls|openssl|tungstenite|tokio-tungstenite|surf|attohttpc'
$denyPlugins = 'tauri-plugin-(http|shell|opener|updater|websocket|process|os|upload|deep-link|single-instance)'
$problems = @()

# Desktop targets only. Tauri pulls in reqwest for Android and iOS, which this
# product does not build (a tree for `--target all` would show it).
$names = @()
foreach ($target in 'x86_64-pc-windows-msvc', 'aarch64-apple-darwin', 'x86_64-unknown-linux-gnu') {
    $tree = cargo tree --workspace --target $target -e normal,build --prefix none 2>&1 | ForEach-Object { "$_" }
    if ($LASTEXITCODE -ne 0) { "no-network: cargo tree failed for $target"; $tree | Select-Object -First 5; exit 1 }
    $targetNames = $tree | ForEach-Object { ($_ -split ' ')[0] } | Sort-Object -Unique
    $names += $targetNames
    foreach ($n in $targetNames) {
        if ($n -match "^($denyCrates)$") { $problems += "networking crate in the dependency tree ($target): $n" }
        if ($n -match "^($denyPlugins)$") { $problems += "Tauri plugin that is not allowed ($target): $n" }
    }
    # tokio without its `net` feature cannot open sockets.
    $tokio = cargo tree --workspace --target $target -e features -i tokio --prefix none 2>&1 | ForEach-Object { "$_" }
    if ($tokio -match 'tokio feature "net"') { $problems += "tokio has its net feature on ($target)" }
}
$names = $names | Sort-Object -Unique

# Capabilities: only the folder and save pickers.
Get-ChildItem app/src-tauri/capabilities -Filter *.json -ErrorAction SilentlyContinue | ForEach-Object {
    $text = Get-Content $_.FullName -Raw
    if ($text -match 'http:|shell:|opener:|updater:|websocket:|fs:|process:') { $problems += "capability grants more than dialogs: $($_.Name)" }
}

# The config must not name an http scope or a remote window URL.
$conf = Get-Content app/src-tauri/tauri.conf.json -Raw
if ($conf -match '"url"\s*:\s*"https?://') { $problems += "tauri.conf.json opens a remote URL" }

if ($problems.Count -gt 0) { $problems | Sort-Object -Unique; "no-network: FAIL"; exit 1 }
"no-network: ok ($($names.Count) crates checked)"
exit 0
