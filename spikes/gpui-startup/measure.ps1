# Launches the spike N times with --exit and reports startup timings.
#   first frame: start of main() until the first frame is presented (from the app)
#   wall:        process launch until it exits after that frame (from this script)
param([int]$Runs = 10, [string]$Exe = "$PSScriptRoot\..\target\release\gpui-startup.exe")

$results = foreach ($i in 1..$Runs) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $out = & $Exe --exit 2>$null
    $sw.Stop()
    $frame = [double](($out | Select-String 'first frame:\s+([\d.]+)').Matches[0].Groups[1].Value)
    [pscustomobject]@{ Run = $i; FirstFrameMs = $frame; WallMs = [math]::Round($sw.Elapsed.TotalMilliseconds, 1) }
}

$results | Format-Table -AutoSize
foreach ($col in 'FirstFrameMs', 'WallMs') {
    $sorted = $results.$col | Sort-Object
    $median = $sorted[[int][math]::Floor($sorted.Count / 2)]
    "{0}: median {1} ms, min {2} ms, max {3} ms" -f $col, $median, $sorted[0], $sorted[-1]
}
"Binary size: {0:N1} MB" -f ((Get-Item $Exe).Length / 1MB)
