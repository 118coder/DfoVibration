# 单发截图: 按进程名找主窗口, Win32 GetWindowRect + PrintWindow 抓帧
param(
    [Parameter(Mandatory=$true)][string]$OutPng,
    [string]$ProcName = "sorahk"
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class Win32Cap3 {
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
[Win32Cap3]::SetProcessDPIAware() | Out-Null
$proc = Get-Process -Name $ProcName -ErrorAction Stop | Where-Object { $_.MainWindowHandle -ne [IntPtr]::Zero } | Select-Object -First 1
if (-not $proc) { Write-Output "NOWINDOW"; exit 1 }
$hwnd = $proc.MainWindowHandle
$rect = New-Object Win32Cap3+RECT
[Win32Cap3]::GetWindowRect($hwnd, [ref]$rect) | Out-Null
$w = $rect.Right - $rect.Left; $h = $rect.Bottom - $rect.Top
if ($w -le 0 -or $h -le 0) { Write-Output "NOSIZE"; exit 1 }
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
[Win32Cap3]::PrintWindow($hwnd, $hdc, 2) | Out-Null
$g.ReleaseHdc($hdc); $g.Dispose()
$bmp.Save($OutPng, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output "SAVED $OutPng ${w}x${h}"
