# Shared helpers for the live app tests (dot-source this). See live-app-test.ps1 for the safety rules.
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
            // A control for the context-menu check: right-clicking this window shows a menu.
            var menu = new ContextMenuStrip(); menu.Items.Add("BabelSteps test menu");
            Form.ContextMenuStrip = menu;
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
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
    delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);

    /// Visible top-level windows, as handles.
    public static long[] VisibleTop() {
        var list = new System.Collections.Generic.List<long>();
        EnumWindows((h, l) => { if (IsWindowVisible(h)) list.Add(h.ToInt64()); return true; }, IntPtr.Zero);
        return list.ToArray();
    }
    /// The visible top-level window titled exactly "BabelSteps" in process pid, or zero.
    public static IntPtr AppWindow(int pid) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == (uint)pid && IsWindowVisible(h)) { var sb = new System.Text.StringBuilder(64); GetWindowText(h, sb, 64); if (sb.ToString() == "BabelSteps") { found = h; return false; } }
            return true;
        }, IntPtr.Zero);
        return found;
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int n);
    /// Window class only (never the title: titles can show private things).
    public static string ClassOf(long h) { var sb = new System.Text.StringBuilder(128); GetClassName(new IntPtr(h), sb, 128); return sb.ToString(); }
    public static int OwnerPid(long h) { uint p; GetWindowThreadProcessId(new IntPtr(h), out p); return (int)p; }
    /// Ask a window to close, as its X button does (WM_CLOSE). No input is sent.
    public static void PostClose(IntPtr h) { PostMessage(h, 0x0010, IntPtr.Zero, IntPtr.Zero); }
    public static int[] ClientPoint(IntPtr h, int cx, int cy) { var p = new POINT { X = cx, Y = cy }; ClientToScreen(h, ref p); return new int[] { p.X, p.Y }; }
    /// Escape, to close a context menu if one opened in the app (the menu has focus then).
    public static void Escape() { var i = new INPUT[] { Key(0x1B, false), Key(0x1B, true) }; SendInput(2, i, Marshal.SizeOf(typeof(INPUT))); }
    public static void CloseTargetMenu() { Form.Invoke(new Action(() => Form.ContextMenuStrip.Close())); }

    /// Click only when our target window is what is under the point.
    public static bool SafeClick(int x, int y) { return SafeClickOn(Handle, x, y, false); }
    /// Click (left or right) only when window `win` is what is under the point.
    public static bool SafeClickOn(IntPtr win, int x, int y, bool right) {
        uint down = right ? 0x0008u : 0x0002u, up = right ? 0x0010u : 0x0004u;
        for (int n = 0; n < 50; n++) {
            var p = new POINT { X = x, Y = y };
            if (GetAncestor(WindowFromPoint(p), 2) == win) {
                POINT o; GetCursorPos(out o);
                int vx = GetSystemMetrics(76), vy = GetSystemMetrics(77), vw = Math.Max(2, GetSystemMetrics(78)), vh = Math.Max(2, GetSystemMetrics(79));
                Func<int, int> nx = px => (int)(((long)(px - vx) * 65535 + (vw - 1) / 2) / (vw - 1));
                Func<int, int> ny = py => (int)(((long)(py - vy) * 65535 + (vh - 1) / 2) / (vh - 1));
                uint abs = 0x0001 | 0x8000 | 0x4000;
                var inputs = new INPUT[] { Mouse(nx(x), ny(y), abs), Mouse(0, 0, down), Mouse(0, 0, up), Mouse(nx(o.X), ny(o.Y), abs) };
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
    if ($script:ws) { try { $script:ws.Dispose() } catch {} }
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
