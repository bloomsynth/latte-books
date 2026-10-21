# Times any desktop app from launch until its window looks finished, using screenshots only,
# so Latte Books spikes and Frappe Books (Electron) are measured the same way.
#   WindowMs: launch until the app has a visible top-level window
#   ReadyMs:  launch until the last change to the window's pixels, with no further change
#             for -SettleMs (blank windows and spinners therefore count as loading)
# The app (every process with the same exe name) is force-closed after each run.
param(
    [Parameter(Mandatory)] [string]$Exe,
    [string[]]$Arguments = @(),
    [int]$Runs = 5,
    [int]$SettleMs = 2000,
    [int]$TimeoutMs = 30000,
    [string]$SaveLastFrame
)

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class Win {
    public delegate bool EnumProc(IntPtr h, IntPtr p);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr p);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    // Largest visible, unowned top-level window belonging to any of the given processes.
    public static IntPtr MainWindow(HashSet<uint> pids) {
        IntPtr best = IntPtr.Zero; long bestArea = 0;
        EnumWindows((h, p) => {
            uint pid; GetWindowThreadProcessId(h, out pid);
            if (pids.Contains(pid) && IsWindowVisible(h) && GetWindow(h, 4) == IntPtr.Zero) {
                RECT r; GetWindowRect(h, out r);
                long area = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
                if (area > 40000 && area > bestArea) { best = h; bestArea = area; }
            }
            return true;
        }, IntPtr.Zero);
        return best;
    }
}
'@

# Window rectangles and screen captures must both be in physical pixels on scaled displays.
[void][Win]::SetProcessDPIAware()

function Get-ProcessTree([int]$RootId) {
    $all = Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId
    $ids = [Collections.Generic.HashSet[uint32]]::new(); [void]$ids.Add($RootId)
    do {
        $added = $false
        foreach ($p in $all) {
            if ($ids.Contains($p.ParentProcessId) -and $ids.Add($p.ProcessId)) { $added = $true }
        }
    } while ($added)
    , $ids
}

function Get-Frame([IntPtr]$Hwnd) {
    $r = New-Object Win+RECT
    [void][Win]::GetWindowRect($Hwnd, [ref]$r)
    $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
    if ($w -le 0 -or $h -le 0) { return $null }
    $bmp = New-Object Drawing.Bitmap $w, $h
    $g = [Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
    $g.Dispose()
    $ms = New-Object IO.MemoryStream
    $small = New-Object Drawing.Bitmap $bmp, ([int]($w / 4)), ([int]($h / 4))
    $small.Save($ms, [Drawing.Imaging.ImageFormat]::Bmp)
    $small.Dispose()
    $hash = [BitConverter]::ToString([Security.Cryptography.SHA1]::Create().ComputeHash($ms.ToArray()))
    @{ Hash = $hash; Bitmap = $bmp }
}

$results = foreach ($run in 1..$Runs) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    # Process.Start without the shell: Start-Process (ShellExecute) adds ~500 ms of its own.
    $psi = [Diagnostics.ProcessStartInfo]::new($Exe, ($Arguments -join ' '))
    $psi.UseShellExecute = $false
    $psi.WorkingDirectory = Split-Path $Exe
    $proc = [Diagnostics.Process]::Start($psi)
    $windowMs = $null; $readyMs = $null; $lastHash = $null; $lastChange = $null; $frame = $null
    # The window can belong to a child process (Electron does this), so also match processes
    # with the same executable name. Get-Process is fast; a CIM process-tree query is not.
    $name = [IO.Path]::GetFileNameWithoutExtension($Exe)
    while ($sw.ElapsedMilliseconds -lt $TimeoutMs) {
        $pids = [Collections.Generic.HashSet[uint32]]::new(); [void]$pids.Add($proc.Id)
        foreach ($p in Get-Process -Name $name -ErrorAction SilentlyContinue) { [void]$pids.Add($p.Id) }
        $hwnd = [Win]::MainWindow($pids)
        if ($hwnd -ne [IntPtr]::Zero) {
            if ($null -eq $windowMs) { $windowMs = $sw.ElapsedMilliseconds }
            $now = $sw.ElapsedMilliseconds
            $f = Get-Frame $hwnd
            if ($f) {
                if ($f.Hash -ne $lastHash) {
                    $lastHash = $f.Hash; $lastChange = $now
                    if ($frame) { $frame.Dispose() }
                    $frame = $f.Bitmap
                } else {
                    $f.Bitmap.Dispose()
                    if ($now - $lastChange -ge $SettleMs) { $readyMs = $lastChange; break }
                }
            }
        }
        Start-Sleep -Milliseconds 10
    }
    $tree = Get-ProcessTree $proc.Id
    foreach ($id in $tree) { Stop-Process -Id $id -Force -ErrorAction SilentlyContinue }
    Get-Process -Name $name -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    if ($SaveLastFrame -and $frame -and $run -eq $Runs) { $frame.Save($SaveLastFrame) }
    if ($frame) { $frame.Dispose() }
    [pscustomobject]@{ Run = $run; WindowMs = $windowMs; ReadyMs = $readyMs }
    Start-Sleep -Seconds 2
}

$results | Format-Table -AutoSize
foreach ($col in 'WindowMs', 'ReadyMs') {
    $sorted = @($results.$col | Where-Object { $_ -ne $null } | Sort-Object)
    if ($sorted.Count) {
        "{0}: median {1} ms, min {2} ms, max {3} ms" -f $col, $sorted[[int][math]::Floor($sorted.Count / 2)], $sorted[0], $sorted[-1]
    }
}
