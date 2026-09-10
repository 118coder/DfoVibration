# 离屏截图: 启动 exe -> 移到屏外 (2600,100) -> PrintWindow(PW_RENDERFULLCONTENT) 抓帧
# 用法: powershell -ExecutionPolicy Bypass -File offscreen_capture.ps1 -ExePath <exe> -OutPng <png>
param(
    [Parameter(Mandatory=$true)][string]$ExePath,
    [Parameter(Mandatory=$true)][string]$OutPng,
    [string]$WorkDir,
    [int]$OffX = 2600,
    [int]$OffY = 100
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32Cap {
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@

[Win32Cap]::SetProcessDPIAware() | Out-Null

# 1. 启动
if ($WorkDir) {
    $proc = Start-Process -FilePath $ExePath -PassThru -WorkingDirectory $WorkDir
} else {
    $proc = Start-Process -FilePath $ExePath -PassThru
}
Start-Sleep -Milliseconds 2500

$proc.Refresh()
$hwnd = $proc.MainWindowHandle
if ($hwnd -eq [IntPtr]::Zero) {
    # 找不到主窗口: 按进程枚举顶层窗口兜底
    Start-Sleep -Milliseconds 1500
    $proc.Refresh()
    $hwnd = $proc.MainWindowHandle
}
if ($hwnd -eq [IntPtr]::Zero) { Write-Error "no main window handle"; exit 1 }

# 2. 移到屏外 (保留原尺寸)
$rect = New-Object Win32Cap+RECT
[Win32Cap]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
$w = $rect.Right - $rect.Left; $h = $rect.Bottom - $rect.Top
[Win32Cap]::SetWindowPos($hwnd, [IntPtr]::Zero, $OffX, $OffY, $w, $h, 0x0004) | Out-Null  # SWP_NOZORDER
Start-Sleep -Milliseconds 1800   # 等 keepalive 重绘

# 3. PrintWindow 抓帧
$rect2 = New-Object Win32Cap+RECT
[Win32Cap]::GetWindowRect($hwnd, [ref]$rect2) | Out-Null
$w2 = $rect2.Right - $rect2.Left; $h2 = $rect2.Bottom - $rect2.Top
$bmp = New-Object System.Drawing.Bitmap($w2, $h2)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$PW_RENDERFULLCONTENT = 2
[Win32Cap]::PrintWindow($hwnd, $hdc, $PW_RENDERFULLCONTENT) | Out-Null
$g.ReleaseHdc($hdc); $g.Dispose()
$bmp.Save($OutPng, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output "SAVED $OutPng ${w2}x${h2}"
