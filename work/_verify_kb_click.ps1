# 离屏点击验证: 启动 exe -> 移屏外 -> PrintWindow 截图 -> PostMessage 点击 -> 再截图
# 点击一律用 PostMessage, 不移动真实光标、不抢焦点。坐标用「截图像素坐标」, 脚本内部换算成客户区坐标。
param(
    [Parameter(Mandatory=$true)][string]$ExePath,
    [Parameter(Mandatory=$true)][string]$OutDir,
    [string]$WorkDir,
    [string]$Clicks,     # "x1,y1;x2,y2;..." 截图坐标序列, 每次点击后截一张图
    [int]$OffX = 2600
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W {
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint f);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
"@
[W]::SetProcessDPIAware() | Out-Null

function Shot($hwnd, $path) {
    $r = New-Object W+RECT
    [W]::GetWindowRect($hwnd, [ref]$r) | Out-Null
    $wd = $r.R - $r.L; $ht = $r.B - $r.T
    $bmp = New-Object System.Drawing.Bitmap($wd, $ht)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [W]::PrintWindow($hwnd, $hdc, 3) | Out-Null   # PW_RENDERFULLCONTENT
    $g.ReleaseHdc($hdc); $g.Dispose()
    $bmp.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

function ClickShot($hwnd, $shotX, $shotY) {
    # 截图像素 -> 客户区坐标: 客户区原点在截图里的偏移 = clientScreen - winTL
    $wr = New-Object W+RECT; [W]::GetWindowRect($hwnd, [ref]$wr) | Out-Null
    $cp = New-Object W+POINT; $cp.X = 0; $cp.Y = 0
    [W]::ClientToScreen($hwnd, [ref]$cp) | Out-Null
    $dx = $cp.X - $wr.L; $dy = $cp.Y - $wr.T
    $cx = $shotX - $dx; $cy = $shotY - $dy
    Write-Host "click shot($shotX,$shotY) -> client($cx,$cy)  [clientOriginOffset=$dx,$dy]"
    $lp = [IntPtr](($cy -shl 16) -bor ($cx -band 0xFFFF))
    [W]::PostMessage($hwnd, 0x0200, [IntPtr]0, $lp) | Out-Null   # WM_MOUSEMOVE
    Start-Sleep -Milliseconds 150
    [W]::PostMessage($hwnd, 0x0201, [IntPtr]1, $lp) | Out-Null   # WM_LBUTTONDOWN (MK_LBUTTON)
    Start-Sleep -Milliseconds 150
    [W]::PostMessage($hwnd, 0x0202, [IntPtr]0, $lp) | Out-Null   # WM_LBUTTONUP
    Start-Sleep -Milliseconds 900
}

if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Path $OutDir | Out-Null }
$proc = Start-Process -FilePath $ExePath -PassThru -WorkingDirectory $WorkDir
Start-Sleep -Milliseconds 2800
$proc.Refresh()
$hwnd = $proc.MainWindowHandle
if ($hwnd -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 1500; $proc.Refresh(); $hwnd = $proc.MainWindowHandle }
if ($hwnd -eq [IntPtr]::Zero) { Write-Error "no main window"; exit 1 }

$r = New-Object W+RECT
[W]::GetWindowRect($hwnd, [ref]$r) | Out-Null
Write-Host "window $($r.L),$($r.T) $($r.R - $r.L)x$($r.B - $r.T)"
[W]::SetWindowPos($hwnd, [IntPtr]::Zero, $OffX, 100, ($r.R - $r.L), 1500, 0x0004) | Out-Null
Start-Sleep -Milliseconds 700

# 伪造聚焦 (不抢真实前台): egui-winit 会吞掉"未聚焦视口的首击"
[W]::PostMessage($hwnd, 0x0006, [IntPtr]1, [IntPtr]0) | Out-Null   # WM_ACTIVATE WA_ACTIVE
[W]::PostMessage($hwnd, 0x0007, [IntPtr]0, [IntPtr]0) | Out-Null   # WM_SETFOCUS
Start-Sleep -Milliseconds 400

Shot $hwnd "$OutDir\shot1_home.png"
$n = 1
if ($Clicks) {
    foreach ($c in $Clicks.Split(';')) {
        $p = $c.Split(',')
        $n++
        ClickShot $hwnd ([int]$p[0]) ([int]$p[1])
        Shot $hwnd ("$OutDir\shot{0}.png" -f $n)
    }
}
Stop-Process -Id $proc.Id -Force
Write-Host "done"
