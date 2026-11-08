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

Add-Type -ReferencedAssemblies System.Windows.Forms, System.Drawing -TypeDefinition @'
using System;
using System.Drawing;
using System.Runtime.InteropServices;
using System.Threading;
using System.Windows.Forms;
public class NoActForm : Form {
    public int Clicks;
    protected override bool ShowWithoutActivation { get { return true; } }
    protected override CreateParams CreateParams { get { CreateParams p = base.CreateParams; p.ExStyle |= 0x08000000 | 0x8; return p; } }
    protected override void OnMouseDown(MouseEventArgs e) { Clicks++; base.OnMouseDown(e); }
}
public static class Live {
    public static NoActForm Form;
    public static IntPtr Handle;
    public static void StartTarget() {
        var ready = new ManualResetEvent(false);
        var t = new Thread(() => {
            Form = new NoActForm();
            Form.Text = "BabelSteps live test target";
            Form.StartPosition = FormStartPosition.Manual;
            Form.Location = new Point(120, 120);
            Form.Size = new Size(420, 260);
            Form.BackColor = Color.FromArgb(0, 160, 0);
            Form.TopMost = true;
            Form.Shown += (s, e) => { Handle = Form.Handle; ready.Set(); };
            Application.Run(Form);
        });
        t.SetApartmentState(ApartmentState.STA);
        t.IsBackground = true;
        t.Start();
        ready.WaitOne(5000);
    }
    public static void StopTarget() { if (Form != null) Form.Invoke(new Action(() => Form.Close())); }

    [StructLayout(LayoutKind.Sequential)] struct POINT { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
    [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort vk, scan; public uint flags, time; public IntPtr extra; }
    [StructLayout(LayoutKind.Explicit)] struct UNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)] struct INPUT { public uint type; public UNION u; }
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint f);
    [DllImport("user32.dll")] static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] i, int size);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern bool RegisterHotKey(IntPtr h, int id, uint mods, uint vk);
    [DllImport("user32.dll")] static extern bool UnregisterHotKey(IntPtr h, int id);

    public static int[] TargetPoint(int fx, int fy) {
        RECT r; GetWindowRect(Handle, out r);
        return new int[] { r.L + (r.R - r.L) * fx / 100, r.T + (r.B - r.T) * fy / 100 };
    }
    static INPUT Mouse(int dx, int dy, uint flags) { var i = new INPUT(); i.type = 0; i.u.mi.dx = dx; i.u.mi.dy = dy; i.u.mi.flags = flags; return i; }
    static INPUT Key(ushort vk, bool up) { var i = new INPUT(); i.type = 1; i.u.ki.vk = vk; i.u.ki.flags = up ? 2u : 0u; return i; }
    /// Click only when our target window is what is under the point.
    public static bool SafeClick(int x, int y) {
        for (int n = 0; n < 50; n++) {
            var p = new POINT { X = x, Y = y };
            if (GetAncestor(WindowFromPoint(p), 2) == Handle) {
                POINT o; GetCursorPos(out o);
                int vx = GetSystemMetrics(76), vy = GetSystemMetrics(77), vw = Math.Max(2, GetSystemMetrics(78)), vh = Math.Max(2, GetSystemMetrics(79));
                Func<int, int> nx = px => (int)(((long)(px - vx) * 65535 + (vw - 1) / 2) / (vw - 1));
                Func<int, int> ny = py => (int)(((long)(py - vy) * 65535 + (vh - 1) / 2) / (vh - 1));
                uint abs = 0x0001 | 0x8000 | 0x4000;
                var inputs = new INPUT[] { Mouse(nx(x), ny(y), abs), Mouse(0, 0, 0x0002), Mouse(0, 0, 0x0004), Mouse(nx(o.X), ny(o.Y), abs) };
                SendInput((uint)inputs.Length, inputs, Marshal.SizeOf(typeof(INPUT)));
                return true;
            }
            Thread.Sleep(100);
        }
        return false;
    }
    /// True if Ctrl+Alt+Shift+vk is free (nobody holds it). Registers and releases at once.
    public static bool ComboFree(uint vk) {
        bool ok = RegisterHotKey(IntPtr.Zero, 0x7A11, 0x1 | 0x2 | 0x4 | 0x4000, vk);
        if (ok) UnregisterHotKey(IntPtr.Zero, 0x7A11);
        return ok;
    }
    /// Press Ctrl+Alt+Shift+vk. Callers first check that BabelSteps holds it, so
    /// the keys are consumed as a hotkey and reach no other app.
    public static void SendCombo(ushort vk) {
        var inputs = new INPUT[] { Key(0x11, false), Key(0x12, false), Key(0x10, false), Key(vk, false), Key(vk, true), Key(0x10, true), Key(0x12, true), Key(0x11, true) };
        SendInput((uint)inputs.Length, inputs, Marshal.SizeOf(typeof(INPUT)));
    }
}
'@

# ---------- CDP ----------
$script:ws = $null; $script:msgId = 0
function Connect-Page {
    $j = Invoke-RestMethod http://127.0.0.1:9333/json -TimeoutSec 3
    $page = @($j | Where-Object { $_.type -eq 'page' -and $_.url -like 'http://tauri.localhost*' })[0]
    if (-not $page) { throw 'no app page' }
    $script:ws = New-Object System.Net.WebSockets.ClientWebSocket
    $script:ws.ConnectAsync([Uri]$page.webSocketDebuggerUrl, [Threading.CancellationToken]::None).Wait()
}
function Eval([string]$body) {
    $script:msgId++
    $expr = "(async()=>{ $body })()"
    $m = @{ id = $script:msgId; method = 'Runtime.evaluate'; params = @{ expression = $expr; returnByValue = $true; awaitPromise = $true } } | ConvertTo-Json -Compress -Depth 5
    $b = [Text.Encoding]::UTF8.GetBytes($m); $ct = [Threading.CancellationToken]::None
    $script:ws.SendAsync([ArraySegment[byte]]$b, 'Text', $true, $ct).Wait()
    while ($true) {
        $ms = New-Object IO.MemoryStream; $buf = New-Object byte[] 65536
        do { $r = $script:ws.ReceiveAsync([ArraySegment[byte]]$buf, $ct).Result; $ms.Write($buf, 0, $r.Count) } while (-not $r.EndOfMessage)
        $o = [Text.Encoding]::UTF8.GetString($ms.ToArray()) | ConvertFrom-Json
        if ($o.id -eq $script:msgId) {
            if ($o.result.exceptionDetails) { return "JSERROR: $($o.result.exceptionDetails.exception.description)" }
            return $o.result.result.value
        }
    }
}
$H = @'
window.__t = {
  btn(t){ const b=[...document.querySelectorAll('button')].find(b=>b.textContent.trim()===t && !b.disabled); if(!b) return false; b.click(); return true; },
  input(label, v){ const l=[...document.querySelectorAll('label')].find(l=>l.textContent.trim().startsWith(label)); const i=l && (l.querySelector('input')); if(!i) return false;
    const set=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set; set.call(i,String(v)); i.dispatchEvent(new Event('input',{bubbles:true})); return true; },
  check(label, want){ const l=[...document.querySelectorAll('label')].find(l=>l.textContent.trim().startsWith(label)); const i=l&&l.querySelector('input[type=checkbox]'); if(!i) return null; if(i.checked!==want) i.click(); return i.checked; },
  text(){ return document.body.innerText; },
  steps(){ const m=document.body.innerText.match(/Steps recorded:\s*(\d+)/); return m?Number(m[1]):-1; },
};
return true;
'@
function Helpers { [void](Eval $H) }
function Wait-Text([string]$needle, [int]$sec = 10) {
    $end = (Get-Date).AddSeconds($sec)
    while ((Get-Date) -lt $end) { $t = Eval "return window.__t ? window.__t.text() : document.body.innerText;"; if ("$t".Contains($needle)) { return $true }; Start-Sleep -Milliseconds 200 }
    return $false
}
function Wait-Stopped([int]$sec = 15) {
    $end = (Get-Date).AddSeconds($sec)
    while ((Get-Date) -lt $end) { $t = "" + (Eval "return window.__t.text();"); if (-not $t.Contains('Steps recorded') -and $t.Contains('Record steps')) { return $true }; Start-Sleep -Milliseconds 200 }
    return $false
}
function App-Procs {
    $app = Get-Process babelsteps-app -ErrorAction SilentlyContinue
    if (-not $app) { return @() }
    $wv = Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -match 'remote-debugging-port=9333' -or $_.CommandLine -match 'babelsteps-app' }
    $root = @($wv | Where-Object { $_.ParentProcessId -eq $app.Id } | ForEach-Object ProcessId)
    $all = @($wv | Where-Object { $root -contains $_.ProcessId -or $root -contains $_.ParentProcessId } | ForEach-Object ProcessId)
    return @($app.Id) + $all
}
function Measure-Mem {
    $ids = App-Procs; $ps = $ids | ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue }
    $ws = ($ps | Measure-Object WorkingSet64 -Sum).Sum / 1MB; $pv = ($ps | Measure-Object PrivateMemorySize64 -Sum).Sum / 1MB
    $appOnly = (Get-Process babelsteps-app).PrivateMemorySize64 / 1MB
    $c1 = 0.0; foreach ($q in $ps) { $c1 += $q.TotalProcessorTime.TotalMilliseconds }; Start-Sleep 5
    $ps2 = $ids | ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue }
    $c2 = 0.0; foreach ($q in $ps2) { $c2 += $q.TotalProcessorTime.TotalMilliseconds }
    return @{ procs = $ids.Count; ws = [math]::Round($ws); priv = [math]::Round($pv); appPriv = [math]::Round($appOnly); cpu = [math]::Round(($c2 - $c1) / 5000 * 100, 1) }
}
function Sockets {
    $ids = App-Procs
    $t = @(Get-NetTCPConnection -ErrorAction SilentlyContinue | Where-Object { $ids -contains $_.OwningProcess -and -not ($_.LocalPort -eq 9333 -and $_.LocalAddress -eq '127.0.0.1') })
    $u = @(Get-NetUDPEndpoint -ErrorAction SilentlyContinue | Where-Object { $ids -contains $_.OwningProcess })
    return "$($ids.Count) processes; TCP other than the debug port: $($t.Count); UDP: $($u.Count)"
}
function Click-Target([int]$n) { for ($i = 0; $i -lt $n; $i++) { $p = [Live]::TargetPoint(30 + 10 * $i, 60); if (-not [Live]::SafeClick($p[0], $p[1])) { throw 'target covered; not clicking' }; Start-Sleep -Milliseconds 800 } }
function Combo([string]$k, [string]$why) {
    $vk = if ($k -like 'F*') { 0x6F + [int]$k.Substring(1) } else { [int][char]$k }
    if ([Live]::ComboFree([uint32]$vk)) { throw "Ctrl+Alt+Shift+$k is not held by BabelSteps; not sending it ($why)" }
    [Live]::SendCombo([uint16]$vk)
}

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
