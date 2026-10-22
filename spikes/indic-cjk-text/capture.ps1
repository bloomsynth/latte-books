# Launches an app, waits for its window to settle, saves a screenshot of the window, then closes it.
#   .\capture.ps1 -Exe ..\target\release\indic-cjk-text.exe -Out gpui.png
#   .\capture.ps1 -Exe ..\target\release\invoice-grid.exe -Arguments --demo -Out grid.png
param([string]$Exe = "$PSScriptRoot\..\target\release\indic-cjk-text.exe", [string]$Arguments = "", [string]$Out = "capture.png", [int]$SettleMs = 2500)

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class Win {
    public delegate bool EnumProc(IntPtr h, IntPtr p);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr p);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    public static IntPtr Find(uint pid) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, p) => {
            uint owner; GetWindowThreadProcessId(h, out owner);
            RECT r; GetWindowRect(h, out r);
            if (owner == pid && IsWindowVisible(h) && r.Right - r.Left > 200) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@
[void][Win]::SetProcessDPIAware()

$process = [Diagnostics.Process]::Start((Resolve-Path $Exe).Path, $Arguments)
try {
    $hwnd = [IntPtr]::Zero
    $deadline = [DateTime]::Now.AddSeconds(15)
    while ($hwnd -eq [IntPtr]::Zero -and [DateTime]::Now -lt $deadline) {
        Start-Sleep -Milliseconds 100
        $hwnd = [Win]::Find([uint32]$process.Id)
    }
    if ($hwnd -eq [IntPtr]::Zero) { throw "no window appeared" }
    [void][Win]::SetForegroundWindow($hwnd)
    Start-Sleep -Milliseconds $SettleMs
    $r = New-Object Win+RECT
    [void][Win]::GetWindowRect($hwnd, [ref]$r)
    $bmp = New-Object Drawing.Bitmap ($r.Right - $r.Left), ($r.Bottom - $r.Top)
    $g = [Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
    # Resolved by PowerShell: .NET would resolve a relative path against its own current directory.
    $bmp.Save($ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Out), [Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
    "saved $Out ($($r.Right - $r.Left)x$($r.Bottom - $r.Top))"
} finally {
    if (-not $process.HasExited) { $process.Kill() }
}
