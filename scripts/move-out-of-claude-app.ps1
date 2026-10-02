<#
.SYNOPSIS
  One-time fix: moves Glowby's data out of the Claude desktop app's private folder.

  Why: the Claude desktop app (Microsoft Store / MSIX version) redirects files
  that programs started FROM it write to AppData into its own private folder
  (%LOCALAPPDATA%\Packages\Claude_...\LocalCache). Glowby was built and first
  started from inside the Claude app, so its hook program and data ended up
  there, where a Glowby started normally (Desktop, Start menu) can't see them.

  What this does (nothing is deleted):
    1. stops Glowby
    2. backs up the normal data folder (if any) to dev.glowby.app.before-move-<time>
    3. copies your data (settings, progress, characters …) from the private copy
       to the normal %APPDATA%\dev.glowby.app
    4. renames the private copies to *.moved-<time> so they stop hiding the real ones
    5. updates the glowby.exe on your Desktop (if there is one) to the new build
    6. starts Glowby again; it installs its hook program itself

  Run it by double-clicking move-out-of-claude-app.cmd in File Explorer
  (NOT from a terminal inside the Claude app, which is redirected too).
#>
$ErrorActionPreference = "Stop"
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$root = Split-Path $PSScriptRoot -Parent
$log = Join-Path $PSScriptRoot "move-out-of-claude-app.log"
function Say($text) { Write-Host $text; Add-Content -Path $log -Value $text -Encoding utf8 }
Set-Content -Path $log -Value "Glowby move, $stamp" -Encoding utf8

# 0. Refuse to run inside the redirected environment (it would just copy the data onto itself).
$probe = Join-Path $env:LOCALAPPDATA "glowby-probe-$stamp.txt"
Set-Content -Path $probe -Value "probe"
$redirected = @(Get-ChildItem "$env:LOCALAPPDATA\Packages\*\LocalCache\Local\$(Split-Path $probe -Leaf)" -ErrorAction SilentlyContinue).Count -gt 0
Remove-Item $probe -Force -ErrorAction SilentlyContinue
if ($redirected) {
  Say "This window runs inside the Claude app, whose AppData is redirected."
  Say "Close it and double-click scripts\move-out-of-claude-app.cmd in File Explorer instead."
  exit 1
}

$private = Get-ChildItem "$env:LOCALAPPDATA\Packages" -Directory -Filter "Claude_*" -ErrorAction SilentlyContinue |
  ForEach-Object { Join-Path $_.FullName "LocalCache" } | Where-Object { Test-Path (Join-Path $_ "Roaming\dev.glowby.app") } | Select-Object -First 1
$real = Join-Path $env:APPDATA "dev.glowby.app"

# 1. stop Glowby
Get-Process glowby -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1
Say "Stopped Glowby."

if ($private) {
  $hiddenData = Join-Path $private "Roaming\dev.glowby.app"
  # 2. back up the normal folder
  if (Test-Path $real) {
    Copy-Item $real "$real.before-move-$stamp" -Recurse
    Say "Backed up the current data to $real.before-move-$stamp"
  }
  # 3. copy your data over
  robocopy $hiddenData $real /E /R:1 /W:1 /NFL /NDL /NJH /NJS /NP | Out-Null
  if ($LASTEXITCODE -ge 8) { Say "Copying failed (robocopy code $LASTEXITCODE). Nothing else was changed."; exit 1 }
  Say "Copied your Glowby data to $real"
  # 4. stop the private copies from hiding the real ones (renamed, not deleted)
  foreach ($dir in $hiddenData, (Join-Path $private "Local\Glowby")) {
    if (-not (Test-Path $dir)) { continue }
    $done = $false
    foreach ($try in 1..5) {
      try { Rename-Item $dir "$(Split-Path $dir -Leaf).moved-$stamp"; $done = $true; break } catch { Start-Sleep -Milliseconds 500 }
    }
    Say ($(if ($done) { "Renamed the private copy: $dir" } else { "Couldn't rename $dir (in use); it's harmless, try again later." }))
  }
} else {
  Say "No private copy found; your data is already in the normal place."
}

# 5. update the Desktop copy, if you made one
$newExe = Join-Path $root "target\release\glowby.exe"
$desktopExe = Join-Path ([Environment]::GetFolderPath("Desktop")) "glowby.exe"
if ((Test-Path $desktopExe) -and (Test-Path $newExe)) {
  Copy-Item $newExe $desktopExe -Force
  Say "Updated $desktopExe to the new version."
}

# 6. start Glowby (it installs its own hook program on start)
$start = if (Test-Path $desktopExe) { $desktopExe } else { $newExe }
Start-Process -FilePath $start
Say "Started $start"
Start-Sleep -Seconds 4
$hook = Join-Path $env:LOCALAPPDATA "Glowby\bin\glowby-hook.exe"
Say "Hook program installed: $(Test-Path $hook)"
Say "Done. You can close this window."
Start-Sleep -Seconds 6
