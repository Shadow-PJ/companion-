<#
.SYNOPSIS
  Measures Glowby's memory and CPU, including all its WebView2 helper processes.

.DESCRIPTION
  WebView2 (the embedded browser) runs as several msedgewebview2.exe processes,
  so looking only at glowby.exe would hide most of the cost. This script finds
  glowby.exe and every process it started, then reports:
    * Private working set: the "Memory" column in Task Manager
    * CPU: average % of ALL cores over the sample (like Task Manager)

.EXAMPLE
  .\scripts\measure.ps1 -Seconds 60 -Label "hidden, idle"
#>
param([int]$Seconds = 30, [string]$Label = "sample")

$all = Get-CimInstance Win32_Process
$root = $all | Where-Object { $_.Name -eq "glowby.exe" } | Select-Object -First 1
if (-not $root) { Write-Error "Glowby isn't running."; exit 1 }

$ids = New-Object 'System.Collections.Generic.HashSet[int]'
[void]$ids.Add([int]$root.ProcessId)
do {
  $added = $false
  foreach ($p in $all) {
    if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $added = $true }
  }
} while ($added)

$cores = [Environment]::ProcessorCount
$start = @{}
foreach ($p in Get-Process -Id ([int[]]@($ids)) -ErrorAction SilentlyContinue) { $start[$p.Id] = $p.TotalProcessorTime.TotalMilliseconds }
Start-Sleep -Seconds $Seconds
$end = Get-Process -Id ([int[]]@($ids)) -ErrorAction SilentlyContinue
$perf = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $ids.Contains([int]$_.IDProcess) }

$rows = foreach ($p in $end) {
  $before = if ($start.ContainsKey($p.Id)) { $start[$p.Id] } else { 0 }
  $cpuMs = $p.TotalProcessorTime.TotalMilliseconds - $before
  $counter = $perf | Where-Object { $_.IDProcess -eq $p.Id } | Select-Object -First 1
  [pscustomobject]@{
    Process        = $p.ProcessName
    PID            = $p.Id
    "Memory (MB)"  = [math]::Round($counter.WorkingSetPrivate / 1MB, 1)
    "CPU (%)"      = [math]::Round($cpuMs / ($Seconds * 1000 * $cores) * 100, 3)
  }
}
$rows | Sort-Object "Memory (MB)" -Descending | Format-Table -AutoSize | Out-String
$memory = ($rows | Measure-Object "Memory (MB)" -Sum).Sum
$cpu = ($rows | Measure-Object "CPU (%)" -Sum).Sum
"[{0}] total memory {1:N1} MB, average CPU {2:N2} % over {3} s ({4} processes, {5} logical cores)" -f $Label, $memory, $cpu, $Seconds, $rows.Count, $cores
