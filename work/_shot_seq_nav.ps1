# ★v24.38 序列宏端到端验证 - 阶段A: 启动→导向→连发页→抓帧 (定坐标用)
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
public class SeqCap {
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
    public static void Key(ushort vk, uint flags) {
        var inputs = new INPUT[1];
        inputs[0].u.ki = new KBINPUT { wVk = vk, dwFlags = flags };
        SendInput(1, inputs, Marshal.SizeOf(typeof(INPUT)));
    }
    public static void Wheel(int x, int y, int delta) {
        uint ax, ay; Norm(x, y, out ax, out ay);
        var inputs = new INPUT[1];
        inputs[0].u.mi = new MOUSEINPUT { dx = (int)ax, dy = (int)ay, mouseData = (uint)delta, dwFlags = 0x0800 };
        SendInput(1, inputs, Marshal.SizeOf(typeof(INPUT)));
    }
    static MOUSEINPUT MI(uint ax, uint ay, uint flags) {
        var mi = new MOUSEINPUT(); mi.dx = (int)ax; mi.dy = (int)ay; mi.dwFlags = flags; return mi;
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
[SeqCap]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$proc = Start-Process -FilePath $ExePath -WorkingDirectory $WorkDir -PassThru
Start-Sleep -Milliseconds 3500
$proc.Refresh()
$hwnd = $proc.MainWindowHandle
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "NOWINDOW"; exit 1 }
[SeqCap]::SetWindowPos($hwnd, [IntPtr]::Zero, 60, 60, 0, 0, 0x0001) | Out-Null
[SeqCap]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 1500

function Capture([string]$name) {
    $rect = New-Object SeqCap+RECT
    [SeqCap]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
    $w = $rect.Right - $rect.Left; $h = $rect.Bottom - $rect.Top
    if ($w -le 0 -or $h -le 0) { Write-Output "SKIP $name"; return }
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [SeqCap]::PrintWindow($hwnd, $hdc, 2) | Out-Null
    $g.ReleaseHdc($hdc); $g.Dispose()
    $bmp.Save((Join-Path $OutDir "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Output "SAVED $name ${w}x${h}"
}
function ClickAt([int]$x, [int]$y) {
    $r = New-Object SeqCap+RECT
    [SeqCap]::GetWindowRect($hwnd, [ref]$r) | Out-Null
    [SeqCap]::Click($r.Left + $x, $r.Top + $y)
    Start-Sleep -Milliseconds 900
    [SeqCap]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 400
}

$rect = New-Object SeqCap+RECT
[SeqCap]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
$W = $rect.Right - $rect.Left

# 关三个首次运行向导
ClickAt ([int]($W * 0.5)) 518
ClickAt ([int]($W * 0.4983)) 535
ClickAt ([int]($W * 0.5)) 673

# 连发映射修改页
ClickAt 110 190
Start-Sleep -Milliseconds 600
# 滚到映射列表 (页面区域点击后 End)
ClickAt 700 700
Start-Sleep -Milliseconds 300
# 滚轮下滚 6 格 (停在映射列表)
[SeqCap]::Wheel(700, 500, -360 * 6)
Start-Sleep -Milliseconds 800
# 点编辑 → 编辑卡自动滚到顶部 → 下滚找序列宏区
ClickAt 1364 651
Start-Sleep -Milliseconds 900
[SeqCap]::Wheel(700, 500, -360 * 3)
Start-Sleep -Milliseconds 700
Capture "edit-seq"
Write-Output "PID $($proc.Id)"
Stop-Process -Id $proc.Id -Force
Write-Output "DONE"
