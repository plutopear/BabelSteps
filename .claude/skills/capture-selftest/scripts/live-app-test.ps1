# Live test of the BabelSteps release app. Needs the owner's OK for every run:
# it records their real screen for about two minutes.
# Safety: app buttons are clicked inside the page (CDP), not with the mouse. Real
# clicks go only to our own test window, and only when it is under the point.
# Shortcut keys are sent only when the BabelSteps app holds that shortcut.
# Settings are backed up first and restored after; the test recording is deleted.
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path
$exe = Join-Path $repo 'target\app-release\release\babelsteps-app.exe'
$settingsFile = Join-Path $env:APPDATA 'app.babelsteps.desktop\settings.json'
$library = Join-Path $env:LOCALAPPDATA 'BabelSteps'
$title = 'BabelSteps live app test'
$scratch = Join-Path $env:TEMP "babelsteps-live-app-test-$PID"; New-Item -ItemType Directory -Force $scratch | Out-Null
$results = New-Object System.Collections.ArrayList
function Check($name, $pass, $detail) { $t = if ($pass) { 'PASS' } else { 'FAIL' }; $l = "$t $name : $detail"; Write-Output $l; [void]$results.Add($l) }
function Info($text) { $l = "info $text"; Write-Output $l; [void]$results.Add($l) }

. (Join-Path $PSScriptRoot 'live-common.ps1')


# ---------- run ----------
$backup = Join-Path $scratch 'settings.backup.json'
$hadSettings = Test-Path $settingsFile
if ($hadSettings) { Copy-Item $settingsFile $backup -Force }
Info "settings file existed before: $hadSettings"
$before = @(Get-ChildItem $library -Directory -ErrorAction SilentlyContinue | ForEach-Object Name)
$app = $null
try {
    if (Get-Process babelsteps-app -ErrorAction SilentlyContinue) { throw 'BabelSteps is already running; not starting a second copy' }
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9333 --remote-debugging-address=127.0.0.1'
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $app = Start-Process -FilePath $exe -PassThru
    $loaded = $false
    while ($sw.Elapsed.TotalSeconds -lt 20 -and -not $loaded) {
        try { if (-not $script:ws) { Connect-Page }; $loaded = (Eval "return !!document.querySelector('h1') && document.body.innerText.includes('Library');") -eq $true } catch { $script:ws = $null; Start-Sleep -Milliseconds 50 }
    }
    $sw.Stop()
    Check 'launch: the Library screen loads' $loaded ("{0:N2} s from launch to the Library screen (includes the debug-port poll, so slightly high)" -f $sw.Elapsed.TotalSeconds)
    Helpers
    Start-Sleep 10
    $m = Measure-Mem
    Info "idle after 10 s: $($m.procs) processes; private memory $($m.priv) MB total, app process alone $($m.appPriv) MB; working set $($m.ws) MB; CPU $($m.cpu)% of one core over 5 s"

    # Settings: keep screenshots for the newest 2 steps; start/stop on Ctrl+Alt+Shift+F9.
    [void](Eval "return window.__t.btn('Settings');"); Start-Sleep -Milliseconds 800
    $lim = Eval "return window.__t.check('Keep screenshots only for the newest steps', true);"
    Start-Sleep -Milliseconds 300
    $k2 = Eval "return window.__t.input('Number of newest steps that keep their screenshot', 2);"
    $f9 = Eval "return window.__t.input('Start or stop recording', 'Ctrl+Alt+Shift+F9');"
    [void](Eval "return window.__t.btn('Save');"); Start-Sleep 1
    $s = Get-Content $settingsFile -Raw
    Check 'settings: saved to disk' ($lim -and $k2 -and $f9 -and $s -match 'F9' -and $s -match '"?keep\w*"?\s*:\s*2') ("file now: " + ($s -replace '\s+', ' '))
    Check 'settings: the new start shortcut is registered at once' (-not [Live]::ComboFree(0x78)) 'Ctrl+Alt+Shift+F9 is held'
    Check 'settings: the old start shortcut is released' ([Live]::ComboFree([uint32][char]'R')) 'Ctrl+Alt+Shift+R is free'

    # Recording 1, from the UI.
    [void](Eval "return window.__t.input('New recording name', '$title');")
    [void](Eval "return window.__t.btn('Create and open');")
    Check 'editor: the new recording opens' (Wait-Text 'Record steps' 8) ''
    [void](Eval "return window.__t.btn('Record steps');"); Start-Sleep -Milliseconds 600
    $keysOff = Eval "const l=[...document.querySelectorAll('label')].find(l=>l.textContent.includes('Record keystrokes')); return l ? !l.querySelector('input').checked : null;"
    Check 'record dialog: keystrokes off by default' ($keysOff -eq $true) ''
    [void](Eval "return window.__t.btn('Start recording');")
    Check 'recording: started from the UI' (Wait-Text 'Steps recorded' 8) ''
    [Live]::StartTarget(); Start-Sleep 1
    Click-Target 2
    [void](Eval "return window.__t.btn('Pause');"); Start-Sleep -Milliseconds 600
    $pausedUi = Wait-Text 'Paused: nothing is being recorded' 3
    Click-Target 1
    [void](Eval "return window.__t.btn('Resume');"); Start-Sleep -Milliseconds 600
    Check 'pause button: pauses and resumes' ($pausedUi -and -not (Wait-Text 'Paused: nothing' 1)) ''
    [void](Eval "return window.__t.input('Add a note to the steps', 'panel note');"); Start-Sleep -Milliseconds 200
    [void](Eval "return window.__t.btn('Add');"); Start-Sleep -Milliseconds 600
    Combo 'P' 'pause'; Start-Sleep -Milliseconds 800
    $pausedKey = Wait-Text 'Paused: nothing is being recorded' 3
    Click-Target 1
    Combo 'P' 'resume'; Start-Sleep -Milliseconds 800
    Check 'pause shortcut: pauses and resumes' ($pausedKey -and -not (Wait-Text 'Paused: nothing' 1)) ''
    Combo 'N' 'note'; Start-Sleep -Milliseconds 800
    $focused = Eval "const a=document.activeElement; return !!a && !!a.closest('label') && a.closest('label').textContent.includes('Add a note');"
    Check 'note shortcut: focuses the note box' ($focused -eq $true) ''
    Click-Target 2
    Info ("while recording: " + (Sockets))
    $m2 = Measure-Mem
    Info "while recording: private memory $($m2.priv) MB total, app process alone $($m2.appPriv) MB; CPU $($m2.cpu)% of one core over 5 s (idle desktop, no clicks in the window)"
    $count1 = Eval "return window.__t.steps();"
    Info "panel step count before stop: $count1 (expected 5: 4 unpaused clicks + 1 note; more if the owner clicked)"
    [void](Eval "return window.__t.btn('Stop recording');")
    $stopped = Wait-Stopped 15
    $notice = Wait-Text 'Kept screenshots for the newest 2 steps' 4
    Check 'stop button: stops and returns to the editor' $stopped ''
    Check 'stop notice: retention notice shown' ($notice -eq $true) 'expects "Kept screenshots for the newest 2 steps"'
    [void](Eval "return window.__t.btn('Dismiss');")

    # Recording 2, from the remapped shortcut; stop with X.
    Combo 'F9' 'remapped start'; Start-Sleep 1
    Check 'remapped shortcut: starts recording' (Wait-Text 'Steps recorded' 8) ''
    Click-Target 1
    Combo 'X' 'stop now'
    Check 'stop-now shortcut: stops' (Wait-Stopped 15) ''

    # Shortcuts back to the defaults; no screenshot limit.
    [void](Eval "return window.__t.btn('Settings');"); Start-Sleep -Milliseconds 800
    [void](Eval "return window.__t.btn('Reset shortcuts to the defaults');")
    [void](Eval "return window.__t.check('Keep screenshots only for the newest steps', false);")
    [void](Eval "return window.__t.btn('Save');"); Start-Sleep 1
    Check 'settings: reset brings back the default shortcut' ((-not [Live]::ComboFree([uint32][char]'R')) -and [Live]::ComboFree(0x78)) 'R held, F9 free'
}
catch { Check 'run: finished' $false "$_" }
finally {
    try { [Live]::StopTarget() } catch {}
    if ($script:ws) { $script:ws.Dispose() }
    $p = Get-Process babelsteps-app -ErrorAction SilentlyContinue
    if ($p) { [void]$p.CloseMainWindow(); Start-Sleep 4; $p.Refresh(); if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force; Info 'app had to be killed' } else { Info 'app closed from its window' } }
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $null
}

# On-disk check of the test recording, then delete it.
$new = @(Get-ChildItem $library -Directory -ErrorAction SilentlyContinue | Where-Object { $before -notcontains $_.Name -and $_.Name -like 'BabelSteps live app test*' })
Check 'disk: one new test recording folder' ($new.Count -eq 1) ("found $($new.Count)")
if ($new.Count -eq 1) {
    $py = @'
import sqlite3, json, os, sys
sys.stdout.reconfigure(encoding='utf-8')
b = sys.argv[1]
c = sqlite3.connect(os.path.join(b, 'recording.db'))
blocks = [json.loads(r[0]) for r in c.execute('select json from blocks order by position')]
def kind(x): return x.get('type') or x.get('kind') or next(iter(x)) if isinstance(x, dict) else '?'
steps = [x for x in blocks if 'title' in json.dumps(x)[:4000]]
refs = []
def walk(o):
    if isinstance(o, dict):
        for k, v in o.items():
            if k == 'screenshot' and isinstance(v, str): refs.append(v)
            else: walk(v)
    elif isinstance(o, list):
        for v in o: walk(v)
walk(blocks)
imgs = ['images/' + f for f in os.listdir(os.path.join(b, 'images'))] if os.path.isdir(os.path.join(b, 'images')) else []
titles = []
def t(o):
    if isinstance(o, dict):
        if 'title' in o and isinstance(o['title'], str): titles.append(o['title'])
        for v in o.values(): t(v)
    elif isinstance(o, list):
        for v in o: t(v)
t(blocks)
print(json.dumps({'blocks': len(blocks), 'refs': len(refs), 'images': len(imgs),
  'unreferenced': len(set(imgs) - set(refs)), 'missing': len(set(refs) - set(imgs)),
  'note_titles': sum(1 for x in titles if 'panel note' in x),
  'target_clicks': sum(1 for x in titles if x.startswith('Left-click') and 'live test target' not in x.lower() and 'powershell' in x.lower()),
  'titles_with_target_window_title': sum(1 for x in titles if 'live test target' in x.lower())}))
'@
    $pyFile = Join-Path $scratch 'inspect.py'; Set-Content $pyFile $py -Encoding ascii
    $r = python -I $pyFile $new[0].FullName | ConvertFrom-Json
    Info "disk: $($r.blocks) blocks, $($r.refs) screenshot references, $($r.images) image files, $($r.unreferenced) unreferenced, $($r.missing) missing; note steps $($r.note_titles); test-window clicks named after PowerShell $($r.target_clicks)"
    Check 'disk: no orphan or missing images' ($r.unreferenced -eq 0 -and $r.missing -eq 0) ''
    Check 'disk: the panel note is a step' ($r.note_titles -ge 1) ''
    Check 'disk: the test window title is in no step title' ($r.titles_with_target_window_title -eq 0) ''
    Remove-Item -LiteralPath $new[0].FullName -Recurse -Force
    Check 'cleanup: test recording deleted' (-not (Test-Path -LiteralPath $new[0].FullName)) ''
}
if ($hadSettings) { Copy-Item $backup $settingsFile -Force; $same = (Get-FileHash $backup).Hash -eq (Get-FileHash $settingsFile).Hash; Remove-Item $backup } else { Remove-Item $settingsFile -ErrorAction SilentlyContinue; $same = -not (Test-Path $settingsFile) }
Check 'cleanup: settings restored to how they were' $same ''
Check 'cleanup: nothing left running' (-not (Get-Process babelsteps-app -ErrorAction SilentlyContinue)) ''
$f = ($results | Where-Object { $_ -like 'FAIL*' }).Count; $p = ($results | Where-Object { $_ -like 'PASS*' }).Count
Remove-Item $scratch -Recurse -Force -ErrorAction SilentlyContinue
Write-Output "SUMMARY $p passed, $f failed"
