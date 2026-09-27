# ★v24.37 手柄组合键面板视觉验证: 启动 exe (空 scratch 配置) → 关向导 → 手柄页抓帧
# 用法: powershell -ExecutionPolicy Bypass -File work/_shot_chord.ps1 -ExePath <exe> -WorkDir <空目录> -OutDir <dir>
# 必须在交互桌面会话运行; SORAHK_NO_AUTO_INJECT=1 防止真注入游戏客户端。
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
public class ShotCap2 {
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
[ShotCap2]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$proc = Start-Process -FilePath $ExePath -WorkingDirectory $WorkDir -PassThru
Start-Sleep -Milliseconds 3500
$proc.Refresh()
$hwnd = $proc.MainWindowHandle
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "NOWINDOW"; exit 1 }
[ShotCap2]::SetWindowPos($hwnd, [IntPtr]::Zero, 60, 60, 0, 0, 0x0001) | Out-Null
[ShotCap2]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 1500

function Capture([string]$name) {
    $rect = New-Object ShotCap2+RECT
    [ShotCap2]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
    $w = $rect.Right - $rect.Left; $h = $rect.Bottom - $rect.Top
    if ($w -le 0 -or $h -le 0) { Write-Output "SKIP $name"; return }
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [ShotCap2]::PrintWindow($hwnd, $hdc, 2) | Out-Null
    $g.ReleaseHdc($hdc); $g.Dispose()
    $bmp.Save((Join-Path $OutDir "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Output "SAVED $name ${w}x${h}"
}
function ClickAt([int]$x, [int]$y) {
    $r = New-Object ShotCap2+RECT
    [ShotCap2]::GetWindowRect($hwnd, [ref]$r) | Out-Null
    [ShotCap2]::Click($r.Left + $x, $r.Top + $y)
    Start-Sleep -Milliseconds 1000
    [ShotCap2]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 500
}

$rect = New-Object ShotCap2+RECT
[ShotCap2]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
$W = $rect.Right - $rect.Left
Write-Output "WINDOW ${W}x$($rect.Bottom - $rect.Top)"

# 关三个首次运行向导
ClickAt ([int]($W * 0.5)) 518
Start-Sleep -Milliseconds 900
ClickAt ([int]($W * 0.4983)) 535
Start-Sleep -Milliseconds 900
ClickAt ([int]($W * 0.5)) 673
Start-Sleep -Milliseconds 900

# 手柄页 (侧边栏第 1 页, y=135)
ClickAt 110 135
Start-Sleep -Milliseconds 800
Capture "page-gamepad-chord-empty"

# 滚到页面底部的组合键面板再抓一张 (PageEnd 滚动: 点击页面区域后按 End)
Add-Type @"
using System; using System.Runtime.InteropServices;
public class KB2 {
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
}
"@
[KB2]::keybd_event(0x23, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 120
[KB2]::keybd_event(0x23, 0, 2, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 800
Capture "page-gamepad-chord-end"

# ★交互流: 点「＋ 添加组合键」→ 组合捕获卡 → 取消 → 回列表
ClickAt 1364 878
Start-Sleep -Milliseconds 800
Capture "page-gamepad-chord-capturing"
ClickAt 1364 878
Start-Sleep -Milliseconds 600
Capture "page-gamepad-chord-back"

Stop-Process -Id $proc.Id -Force
Write-Output "DONE"
