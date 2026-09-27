# ★v24.38 序列宏录制端到端: 编辑→录制 J(150ms)/K(100ms)→停止并填入→断言 Config.toml
param(
    [Parameter(Mandatory=$true)][string]$ExePath,
    [Parameter(Mandatory=$true)][string]$WorkDir,
    [Parameter(Mandatory=$true)][string]$OutDir
)
$ErrorActionPreference = "Stop"
$env:SORAHK_NO_AUTO_INJECT = "1"
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class E2eCap {
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int idx);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)]
    public struct MOUSEINPUT { public int dx, dy; public uint mouseData, dwFlags, time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)]
    public struct KBINPUT { public ushort wVk, wScan; public uint dwFlags, time; public IntPtr dwExtraInfo; public uint padding1, padding2; }
    [StructLayout(LayoutKind.Explicit)]
    public struct UNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KBINPUT ki; }
    [StructLayout(LayoutKind.Sequential)]
    public struct INPUT { public uint type; public UNION u; }
    public static void Click(int x, int y) {
        uint ax, ay; Norm(x, y, out ax, out ay);
        var inputs = new INPUT[3];
        inputs[0].u.mi = MI(ax, ay, 0x8001);
        inputs[1].u.mi = MI(ax, ay, 0x0002);
        inputs[2].u.mi = MI(ax, ay, 0x0004);
        SendInput(3, inputs, Marshal.SizeOf(typeof(INPUT)));
    }
    public static void RawKey(ushort vk, uint flags) {
        var inputs = new INPUT[1];
        inputs[0].u.ki = new KBINPUT { wVk = vk, dwFlags = flags };
        SendInput(1, inputs, Marshal.SizeOf(typeof(INPUT)));
    }
    static MOUSEINPUT MI(uint ax, uint ay, uint flags) {
        var mi = new MOUSEINPUT(); mi.dx = (int)ax; mi.dy = (int)ay; mi.dwFlags = flags; return mi;
    }
    public static void Drag(int x1, int y1, int x2, int y2) {
        uint ax, ay; Norm(x1, y1, out ax, out ay);
        var down = new INPUT[1];
        down[0].u.mi = new MOUSEINPUT { dx = (int)ax, dy = (int)ay, dwFlags = 0x8001 | 0x0002 };
        SendInput(1, down, Marshal.SizeOf(typeof(INPUT)));
        SleepMs(120);
        int steps = 24;
        for (int i = 1; i <= steps; i++) {
            int x = x1 + (x2 - x1) * i / steps;
            int y = y1 + (y2 - y1) * i / steps;
            Norm(x, y, out ax, out ay);
            var mv = new INPUT[1];
            mv[0].u.mi = new MOUSEINPUT { dx = (int)ax, dy = (int)ay, dwFlags = 0x8001 };
            SendInput(1, mv, Marshal.SizeOf(typeof(INPUT)));
            SleepMs(30);
        }
        SleepMs(120);
        Norm(x2, y2, out ax, out ay);
        var up = new INPUT[1];
        up[0].u.mi = new MOUSEINPUT { dx = (int)ax, dy = (int)ay, dwFlags = 0x8001 | 0x0004 };
        SendInput(1, up, Marshal.SizeOf(typeof(INPUT)));
    }
    static void SleepMs(int ms) { System.Threading.Thread.Sleep(ms); }
    public static void Wheel(int x, int y, int delta) {
        uint ax, ay; Norm(x, y, out ax, out ay);
        var inputs = new INPUT[1];
        inputs[0].u.mi = new MOUSEINPUT { dx = (int)ax, dy = (int)ay, mouseData = (uint)delta, dwFlags = 0x0800 };
        SendInput(1, inputs, Marshal.SizeOf(typeof(INPUT)));
    }
    static void Norm(int x, int y, out uint ax, out uint ay) {
        int vx = GetSystemMetrics(76), vy = GetSystemMetrics(77);
        int vw = GetSystemMetrics(78), vh = GetSystemMetrics(79);
        ax = (uint)(((long)(x - vx) * 65535) / Math.Max(1, vw - 1));
        ay = (uint)(((long)(y - vy) * 65535) / Math.Max(1, vh - 1));
    }
    [DllImport("user32.dll", SetLastError = true)]
    static extern uint SendInput(uint n, INPUT[] inputs, int size);
}
"@
[E2eCap]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$proc = Start-Process -FilePath $ExePath -WorkingDirectory $WorkDir -PassThru
Start-Sleep -Milliseconds 3500
$proc.Refresh()
$hwnd = $proc.MainWindowHandle
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "NOWINDOW"; exit 1 }
[E2eCap]::SetWindowPos($hwnd, [IntPtr]::Zero, 60, 40, 1475, 1500, 0x0001) | Out-Null
[E2eCap]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 1500
$proc.Refresh()
if ($proc.MainWindowHandle -ne [IntPtr]::Zero) { $hwnd = $proc.MainWindowHandle }

function Capture([string]$name) {
    $rect = New-Object E2eCap+RECT
    [E2eCap]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
    $w = $rect.Right - $rect.Left; $h = $rect.Bottom - $rect.Top
    if ($w -le 0 -or $h -le 0) { Write-Output "SKIP $name"; return }
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [E2eCap]::PrintWindow($hwnd, $hdc, 2) | Out-Null
    $g.ReleaseHdc($hdc); $g.Dispose()
    $bmp.Save((Join-Path $OutDir "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Output "SAVED $name ${w}x${h}"
}
function ClickAt([int]$x, [int]$y) {
    $r = New-Object E2eCap+RECT
    [E2eCap]::GetWindowRect($hwnd, [ref]$r) | Out-Null
    [E2eCap]::Click($r.Left + $x, $r.Top + $y)
    Start-Sleep -Milliseconds 900
    [E2eCap]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 400
}

# 窗口拉高到 ~1440 (拖右下角), 页面内容免滚动全可见
[E2eCap]::Drag(1470, 945, 2000, 1500)
Start-Sleep -Milliseconds 800

# 导航: 连发页 → 折叠「键盘」大卡 (页面缩短, 列表上移进视野) → 编辑 (Q→Q 行)
ClickAt 110 190
Start-Sleep -Milliseconds 1000
ClickAt 400 776
Start-Sleep -Milliseconds 900
Capture "nav-2"
ClickAt 1364 907
Start-Sleep -Milliseconds 1500
# 关可能被误开的下拉 + 连拖滚动条到底
[E2eCap]::RawKey(0x1B, 0)
Start-Sleep -Milliseconds 100
[E2eCap]::RawKey(0x1B, 2)
Start-Sleep -Milliseconds 400
foreach ($i in 1..4) {
    [E2eCap]::Drag(1445, 300, 1445, 840)
    Start-Sleep -Milliseconds 600
}
Capture "scrolled"

# 真实敲键: J 按住 150ms → 停 250ms → K 按住 100ms (keybd_event, 无 marker = 真实键)
[E2eCap]::RawKey(0x4A, 0)
Start-Sleep -Milliseconds 150
[E2eCap]::RawKey(0x4A, 2)
Start-Sleep -Milliseconds 250
[E2eCap]::RawKey(0x4B, 0)
Start-Sleep -Milliseconds 100
[E2eCap]::RawKey(0x4B, 2)
Start-Sleep -Milliseconds 300
Capture "recording"

# 点「■ 停止并填入」 (录制态按钮右移: 估 700,430)
ClickAt 700 430
Start-Sleep -Milliseconds 900
Capture "after-stop"

# 落盘断言素材: 复制 Config.toml + 结束
Copy-Item (Join-Path $WorkDir "Config.toml") (Join-Path $OutDir "Config-after.toml") -Force
Stop-Process -Id $proc.Id -Force
Write-Output "DONE"
