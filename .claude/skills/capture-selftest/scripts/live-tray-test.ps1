# Live test of "Keep running in the tray" (Q21) on the BabelSteps release app.
# Needs the owner's OK for every run: it records their real screen for about a
# minute and the app window takes focus a few times.
# Safety: app buttons are clicked inside the page (CDP), not with the mouse. Real
# left clicks go only to our own test window, and only when it is under the point.
# Right clicks (the context-menu check) go only to our test window or the
# BabelSteps window, only when it is under the point, and never while recording.
# Shortcut keys are sent only when the BabelSteps app holds that shortcut.
# The window is closed with WM_CLOSE (what its X button sends), not with input.
# Settings are backed up first and restored after; test recordings are deleted.
# Not covered: the tray menu and double-clicking the tray icon (they need real
# clicks on the taskbar), and Windows sign-out or shutdown during a recording.
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..\..')).Path
$exe = Join-Path $repo 'target\app-release\release\babelsteps-app.exe'
$settingsFile = Join-Path $env:APPDATA 'app.babelsteps.desktop\settings.json'
$library = Join-Path $env:LOCALAPPDATA 'BabelSteps'
$scratch = Join-Path $env:TEMP "babelsteps-live-tray-test-$PID"; New-Item -ItemType Directory -Force $scratch | Out-Null
$results = New-Object System.Collections.ArrayList
function Check($name, $pass, $detail) { $t = if ($pass) { 'PASS' } else { 'FAIL' }; $l = "$t $name : $detail"; Write-Output $l; [void]$results.Add($l) }
function Info($text) { $l = "info $text"; Write-Output $l; [void]$results.Add($l) }
. (Join-Path $PSScriptRoot 'live-common.ps1')

function Dirs { @(Get-ChildItem $library -Directory -ErrorAction SilentlyContinue | ForEach-Object Name) }
function Wait-Window([int]$appPid, [int]$sec) {
    $end = (Get-Date).AddSeconds($sec)
    while ((Get-Date) -lt $end) { $w = [Live]::AppWindow($appPid); if ($w -ne [IntPtr]::Zero) { return $w }; Start-Sleep -Milliseconds 200 }
    return [IntPtr]::Zero
}
function Connect-Retry([int]$sec = 12) {
    $script:ws = $null
    $end = (Get-Date).AddSeconds($sec)
    while ((Get-Date) -lt $end) {
        try { Connect-Page; if ((Eval "return document.readyState === 'complete' && !!document.querySelector('.topbar');") -eq $true) { Helpers; return $true } } catch { }
        Start-Sleep -Milliseconds 200
    }
    return $false
}
function Drop-Page { if ($script:ws) { try { $script:ws.Dispose() } catch {} }; $script:ws = $null }
# Right-click a point and return the new visible top-level windows owned by $owners.
function Menu-Windows([IntPtr]$win, [int]$cx, [int]$cy, [int[]]$owners) {
    $before = [Live]::VisibleTop()
    $pt = [Live]::ClientPoint($win, $cx, $cy)
    if (-not [Live]::SafeClickOn($win, $pt[0], $pt[1], $true)) { throw 'window covered; not right-clicking' }
    Start-Sleep -Milliseconds 900
    return @([Live]::VisibleTop() | Where-Object { $before -notcontains $_ -and $owners -contains [Live]::OwnerPid($_) })
}
function Set-Close([int]$index) {
    [void](Eval "return window.__t.btn('Settings');"); Start-Sleep -Milliseconds 800
    $ok = Eval "const r=[...document.querySelectorAll('input[name=close]')]; if(r.length!==2) return false; r[$index].click(); return r[$index].checked;"
    [void](Eval "return window.__t.btn('Save');"); Start-Sleep 1
    return $ok
}
function Inspect([string]$dir) {
    $py = @'
import sqlite3, json, os, sys
sys.stdout.reconfigure(encoding='utf-8')
b = sys.argv[1]
c = sqlite3.connect(os.path.join(b, 'recording.db'))
blocks = [json.loads(r[0]) for r in c.execute('select json from blocks order by position')]
refs, titles = [], []
def walk(o):
    if isinstance(o, dict):
        for k, v in o.items():
            if k == 'screenshot' and isinstance(v, str): refs.append(v)
            elif k == 'title' and isinstance(v, str): titles.append(v)
            else: walk(v)
    elif isinstance(o, list):
        for v in o: walk(v)
walk(blocks)
d = os.path.join(b, 'images')
imgs = ['images/' + f for f in os.listdir(d)] if os.path.isdir(d) else []
print(json.dumps({'blocks': len(blocks), 'refs': len(refs), 'images': len(imgs),
  'unreferenced': len(set(imgs) - set(refs)), 'missing': len(set(refs) - set(imgs)),
  'note': sum(1 for t in titles if 'tray note' in t),
  'clicks': sum(1 for t in titles if t.lower().startswith('left-click'))}))
'@
    $pyFile = Join-Path $scratch 'inspect.py'; Set-Content $pyFile $py -Encoding ascii
    return (python -I $pyFile $dir | ConvertFrom-Json)
}

$backup = Join-Path $scratch 'settings.backup.json'
$hadSettings = Test-Path $settingsFile
if ($hadSettings) { Copy-Item $settingsFile $backup -Force }
$before = Dirs
$appPid = 0
$rec1 = $null
try {
    if (Get-Process babelsteps-app -ErrorAction SilentlyContinue) { throw 'BabelSteps is already running; not starting a second copy' }
    if (-not [Live]::ComboFree([uint32][char]'R')) { throw 'Ctrl+Alt+Shift+R is held by another app' }
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9333 --remote-debugging-address=127.0.0.1'
    $app = Start-Process -FilePath $exe -PassThru
    $appPid = $app.Id
    Check 'launch: the window loads' (Connect-Retry 20) ''
    $win0 = Wait-Window $appPid 5
    Start-Sleep 8
    $m0 = Measure-Mem
    Info "window open, idle: $($m0.procs) processes; private memory $($m0.priv) MB total, app process alone $($m0.appPriv) MB; working set $($m0.ws) MB; CPU $($m0.cpu)% of one core"

    # Context-menu check. Control first: our own test window has a menu, so the check must see it.
    [Live]::StartTarget(); Start-Sleep 1
    $ctl = Menu-Windows ([Live]::Handle) 100 100 @($PID)
    Check 'control: the context-menu check sees a menu when there is one' ($ctl.Count -ge 1) ("$($ctl.Count) new window(s) " + (($ctl | ForEach-Object { [Live]::ClassOf($_) }) -join ', '))
    try { [Live]::CloseTargetMenu() } catch {}
    Start-Sleep -Milliseconds 300
    $ids = App-Procs
    $m = Menu-Windows $win0 900 15 $ids
    if ($m.Count) { [Live]::Escape() }
    Check 'start-up window: hardened (no browser context menu)' ($m.Count -eq 0) ("$($m.Count) new window(s) " + (($m | ForEach-Object { [Live]::ClassOf($_) }) -join ', '))

    # Choose "Keep running in the tray".
    $ok = Set-Close 1
    $s = Get-Content $settingsFile -Raw
    Check 'settings: tray choice saved' ($ok -and $s -match '"closeToTray"\s*:\s*true') ''

    # Close the window: the app must stay, with no window and no browser engine.
    Drop-Page
    [Live]::PostClose($win0); Start-Sleep 3
    $alive = [bool](Get-Process -Id $appPid -ErrorAction SilentlyContinue)
    Check 'close (tray mode): the app keeps running' $alive ''
    Check 'close (tray mode): the window is gone' ([Live]::AppWindow($appPid) -eq [IntPtr]::Zero) ''
    Start-Sleep 12
    $m1 = Measure-Mem
    Info "in the tray, idle 15 s after close: $($m1.procs) processes; private memory $($m1.priv) MB total, app process alone $($m1.appPriv) MB; working set $($m1.ws) MB; CPU $($m1.cpu)% of one core over 5 s"
    Check 'D1: idle in the tray under 50 MB' ($m1.priv -lt 50) "private memory $($m1.priv) MB across $($m1.procs) process(es)"
    Info ("in the tray: " + (Sockets))

    # The note shortcut does nothing when not recording.
    Combo 'N' 'note while idle'; Start-Sleep 3
    Check 'note shortcut while idle: the window stays closed' ([Live]::AppWindow($appPid) -eq [IntPtr]::Zero) ''

    # Start from the shortcut with the window closed: a new recording.
    $b1 = Dirs
    Combo 'R' 'start with the window closed'; Start-Sleep 2
    $n1 = @(Dirs | Where-Object { $b1 -notcontains $_ })
    Check 'start shortcut, window closed: a new recording starts' ($n1.Count -eq 1) "$($n1.Count) new folder(s)"
    if ($n1.Count -eq 1) { $rec1 = Join-Path $library $n1[0] }
    Click-Target 2

    # The note shortcut rebuilds the window on the note box.
    Combo 'N' 'note: rebuild the window'
    $win1 = Wait-Window $appPid 10
    Check 'note shortcut: the window comes back' ($win1 -ne [IntPtr]::Zero) ''
    $conn = Connect-Retry 12
    $panel = $conn -and (Wait-Text 'Steps recorded' 6)
    Check 'rebuilt window: shows the running recording' $panel ''
    Start-Sleep -Milliseconds 800
    $focused = Eval "const a=document.activeElement; return !!a && !!a.closest('label') && a.closest('label').textContent.includes('Add a note');"
    $hash = Eval "return location.hash;"
    Check 'rebuilt window: the note box has the cursor' ($focused -eq $true) "hash now '$hash'"
    [void](Eval "return window.__t.input('Add a note to the steps', 'tray note');"); Start-Sleep -Milliseconds 200
    [void](Eval "return window.__t.btn('Add');"); Start-Sleep -Milliseconds 800

    # Close while recording (tray mode): stops and saves, app stays.
    Drop-Page
    [Live]::PostClose($win1); Start-Sleep 4
    Check 'close while recording (tray mode): the app keeps running' ([bool](Get-Process -Id $appPid -ErrorAction SilentlyContinue)) ''
    Combo 'N' 'is it still recording?'; Start-Sleep 3
    Check 'close while recording: the recording stopped (note shortcut opens nothing)' ([Live]::AppWindow($appPid) -eq [IntPtr]::Zero) ''
    if ($rec1) {
        $r = Inspect $rec1
        Info "recording 1 on disk: $($r.blocks) blocks, $($r.clicks) click steps, note steps $($r.note), $($r.images) images, $($r.unreferenced) unreferenced, $($r.missing) missing"
        Check 'recording 1 on disk: clicks and the note were saved' ($r.clicks -ge 2 -and $r.note -ge 1) ''
        Check 'recording 1 on disk: no orphan or missing images' ($r.unreferenced -eq 0 -and $r.missing -eq 0) ''
    }

    # Start and stop with the window closed, then start again: the second start
    # must make a new recording, not add steps to the stopped one.
    $b2 = Dirs
    Combo 'R' 'start recording 2'; Start-Sleep 1.5
    Click-Target 1
    Combo 'X' 'stop recording 2 with the window closed'; Start-Sleep 2
    Combo 'R' 'start recording 3'; Start-Sleep 1.5
    $n2 = @(Dirs | Where-Object { $b2 -notcontains $_ })
    Check 'stop then start, window closed: the second start makes a new recording' ($n2.Count -eq 2) "$($n2.Count) new folder(s), expected 2"

    # Get a window back during recording 3; stop it with X while the window is open.
    Combo 'N' 'open the window'
    $win2 = Wait-Window $appPid 10
    $conn = Connect-Retry 12
    Combo 'X' 'stop now'
    Check 'stop-now shortcut with a rebuilt window: stops and shows the editor' ($conn -and (Wait-Stopped 15)) ''

    # The rebuilt window is hardened like the first one (checked while not recording).
    $ids = App-Procs
    $m = Menu-Windows $win2 900 15 $ids
    if ($m.Count) { [Live]::Escape() }
    Check 'rebuilt window: hardened (no browser context menu)' ($m.Count -eq 0) ("$($m.Count) new window(s) " + (($m | ForEach-Object { [Live]::ClassOf($_) }) -join ', '))

    # Switch back to "Quit": closing must now end the app.
    $ok = Set-Close 0
    $s = Get-Content $settingsFile -Raw
    Check 'settings: quit choice saved' ($ok -and $s -match '"closeToTray"\s*:\s*false') ''
    Drop-Page
    [Live]::PostClose($win2)
    $p = Get-Process -Id $appPid -ErrorAction SilentlyContinue
    $exited = $true; if ($p) { $exited = $p.WaitForExit(8000) }
    Check 'close (quit mode): the app quits' $exited ''
}
catch { Check 'run: finished' $false "$_" }
finally {
    try { [Live]::StopTarget() } catch {}
    Drop-Page
    $p = Get-Process babelsteps-app -ErrorAction SilentlyContinue
    if ($p) { Stop-Process -Id $p.Id -Force; Info 'app had to be killed' }
    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $null
}

# Delete the recordings this test made (folders that are new and named "Recording...").
$made = @(Dirs | Where-Object { $before -notcontains $_ -and $_ -like 'Recording*' })
Info "test recordings to delete: $($made.Count) (expected 3)"
foreach ($d in $made) { Remove-Item -LiteralPath (Join-Path $library $d) -Recurse -Force }
Check 'cleanup: test recordings deleted' (@($made | Where-Object { Test-Path -LiteralPath (Join-Path $library $_) }).Count -eq 0) ''
if ($hadSettings) { Copy-Item $backup $settingsFile -Force; $same = (Get-FileHash $backup).Hash -eq (Get-FileHash $settingsFile).Hash } else { Remove-Item $settingsFile -ErrorAction SilentlyContinue; $same = -not (Test-Path $settingsFile) }
Check 'cleanup: settings restored to how they were' $same ''
Check 'cleanup: nothing left running' (-not (Get-Process babelsteps-app -ErrorAction SilentlyContinue)) ''
$f = ($results | Where-Object { $_ -like 'FAIL*' }).Count; $p = ($results | Where-Object { $_ -like 'PASS*' }).Count
Remove-Item $scratch -Recurse -Force -ErrorAction SilentlyContinue
Write-Output "SUMMARY $p passed, $f failed"
