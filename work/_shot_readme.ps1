# README 截图自动化: 启动 exe (空 scratch 配置=浅色全新态) → 关向导 → 逐页抓帧 → 暗色 → 设置弹窗
# 用法: powershell -ExecutionPolicy Bypass -File work/_shot_readme.ps1 -ExePath <exe> -WorkDir <空目录> -OutDir <dir>
# 依赖交互桌面会话 (SendInput); 会话锁定时抓帧会重复 (脚本输出 WARN)。
param(
    [Parameter(Mandatory=$true)][string]$ExePath,
    [Parameter(Mandatory=$true)][string]$WorkDir,
    [Parameter(Mandatory=$true)][string]$OutDir
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class ShotCap {
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
    public static void Esc() {
        var inputs = new INPUT[2];
        inputs[0].u.ki = KB(0x1B, 0);
        inputs[1].u.ki = KB(0x1B, 2);
        SendInput(2, inputs, Marshal.SizeOf(typeof(INPUT)));
    }
    static KBINPUT KB(ushort vk, uint flags) {
        var ki = new KBINPUT(); ki.wVk = vk; ki.dwFlags = flags; return ki;
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
[ShotCap]::SetProcessDPIAware() | Out-Null
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$proc = Start-Process -FilePath $ExePath -WorkingDirectory $WorkDir -PassThru
Start-Sleep -Milliseconds 3500
$proc.Refresh()
$hwnd = $proc.MainWindowHandle
if ($hwnd -eq [IntPtr]::Zero) { Write-Output "NOWINDOW"; exit 1 }
[ShotCap]::SetWindowPos($hwnd, [IntPtr]::Zero, 60, 60, 0, 0, 0x0001) | Out-Null
[ShotCap]::SetForegroundWindow($hwnd) | Out-Null
Start-Sleep -Milliseconds 1500

function Capture([string]$name) {
    $rect = New-Object ShotCap+RECT
    [ShotCap]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
    $w = $rect.Right - $rect.Left; $h = $rect.Bottom - $rect.Top
    if ($w -le 0 -or $h -le 0) { Write-Output "SKIP $name"; return }
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [ShotCap]::PrintWindow($hwnd, $hdc, 2) | Out-Null
    $g.ReleaseHdc($hdc); $g.Dispose()
    $bmp.Save((Join-Path $OutDir "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Output "SAVED $name ${w}x${h}"
}
function ClickAt([int]$x, [int]$y) {
    $r = New-Object ShotCap+RECT
    [ShotCap]::GetWindowRect($hwnd, [ref]$r) | Out-Null
    [ShotCap]::Click($r.Left + $x, $r.Top + $y)
    Start-Sleep -Milliseconds 1000
    [ShotCap]::SetForegroundWindow($hwnd) | Out-Null
    Start-Sleep -Milliseconds 500
}

$rect = New-Object ShotCap+RECT
[ShotCap]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
$W = $rect.Right - $rect.Left
Write-Output "WINDOW ${W}x$($rect.Bottom - $rect.Top)"

# 1. 关首次运行向导: 点击「是, 我是 DFO 玩家」(1475x950 布局中心 738,518)
ClickAt ([int]($W * 0.5)) 518
Start-Sleep -Milliseconds 900
# 第二个向导: S1/S4 路线选择 → 点推荐 (S1 ACT 老版本)
ClickAt ([int]($W * 0.4983)) 535
Start-Sleep -Milliseconds 900
# 第三个向导: 欢迎使用 → 开始使用
ClickAt ([int]($W * 0.5)) 673
Start-Sleep -Milliseconds 900

# 2. 侧边栏五页 (浅色): x=110, y = 135/190/244/299/354
$pages = @(
    @{ n = "page-gamepad";   y = 135 },
    @{ n = "page-turbo";     y = 190 },
    @{ n = "page-whitelist"; y = 244 },
    @{ n = "page-vibration"; y = 299 },
    @{ n = "page-jobs";      y = 354 }
)
$hashes = @{}
foreach ($p in $pages) {
    ClickAt 110 $p.y
    Capture $p.n
    $f = Join-Path $OutDir "$($p.n).png"
    if (Test-Path $f) { $hashes[$p.n] = (Get-FileHash $f -Algorithm MD5).Hash }
}
$prev = ""
foreach ($p in $pages) {
    $h = $hashes[$p.n]; if (-not $h) { continue }
    if ($h -eq $prev) { Write-Output "WARN: $($p.n) 与上一页画面相同" }
    $prev = $h
}

# 3. 暗色主题 (月亮 1210,28) → 震动页/手柄页暗色
ClickAt 1210 28
ClickAt 110 299
Capture "page-vibration-dark"
ClickAt 110 135
Capture "page-gamepad-dark"
# 切回浅色 (scratch 配置无所谓, 保持截图一致性)
ClickAt 1210 28

# 4. 设置弹窗 (Settings 胶囊 900,28) → 抓帧 → ESC 关闭
ClickAt 900 28
Start-Sleep -Milliseconds 700
Capture "dialog-settings"
[ShotCap]::Esc()
Start-Sleep -Milliseconds 500

Stop-Process -Id $proc.Id -Force
Write-Output "DONE"
