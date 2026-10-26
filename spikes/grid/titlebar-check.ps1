# Starts the spike with each title bar variant, screenshots the window including its frame and probes
# WM_NCHITTEST at the caption buttons, the drag area, the top edge and the client area.
param([string[]]$Variants = @('native', 'custom', 'hybrid'))

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class W {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int s);
}
'@

$codes = @{ 1 = 'HTCLIENT'; 2 = 'HTCAPTION'; 8 = 'HTMINBUTTON'; 9 = 'HTMAXBUTTON'; 20 = 'HTCLOSE'; 12 = 'HTTOP'; 13 = 'HTTOPLEFT'; 14 = 'HTTOPRIGHT' }
$exe = Join-Path $PSScriptRoot 'target\release\spike-grid.exe'
foreach ($v in $Variants) {
    $p = Start-Process $exe -ArgumentList '--titlebar', $v, '--overlay', 'off', '--start-item', '3000', '--nudge', '120' -WorkingDirectory $PSScriptRoot -PassThru
    $h = [IntPtr]::Zero
    for ($i = 0; $i -lt 40 -and $h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 250; $h = [W]::FindWindowW([NullString]::Value, "Spike $([char]0x00B7) grid") }
    Start-Sleep 3
    [W]::SetForegroundWindow($h) | Out-Null
    Start-Sleep -Milliseconds 500
    # Visible bounds without the invisible resize borders (DWMWA_EXTENDED_FRAME_BOUNDS = 9).
    $r = New-Object W+RECT
    [W]::DwmGetWindowAttribute($h, 9, [ref]$r, 16) | Out-Null
    $w = $r.R - $r.L; $hh = $r.B - $r.T
    $bmp = New-Object System.Drawing.Bitmap $w, $hh
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
    $out = Join-Path $PSScriptRoot "results\titlebar-$v.png"
    $bmp.Save($out); $g.Dispose(); $bmp.Dispose()
    "$v : window $w x $hh, saved $out"
    $probes = [ordered]@{
        'close button'    = @(($r.R - 23), ($r.T + 20))
        'maximize button' = @(($r.R - 69), ($r.T + 20))
        'minimize button' = @(($r.R - 115), ($r.T + 20))
        'toolbar gap'     = @(($r.L + 1100), ($r.T + 24))
        'top edge'        = @(($r.L + 1100), ($r.T + 1))
        'gallery'         = @(($r.L + 800), ($r.T + 400))
    }
    foreach ($k in $probes.Keys) {
        $x, $y = $probes[$k]
        $lp = [IntPtr](($y -shl 16) -bor ($x -band 0xffff))
        $code = [int][W]::SendMessageW($h, 0x84, [IntPtr]::Zero, $lp)
        "  {0,-16} -> {1}" -f $k, ($codes[$code] ?? $code)
    }
    Stop-Process -Id $p.Id
    Start-Sleep 1
}
