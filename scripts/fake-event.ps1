<#
.SYNOPSIS
  Sends a pretend Claude Code hook event to Glowby, without running Claude Code.
  It runs the real glowby-hook.exe, so it tests the whole path.

.EXAMPLE
  .\scripts\fake-event.ps1 -Event UserPromptSubmit
  .\scripts\fake-event.ps1 -Event PreToolUse -Tool Edit -Target src\main.rs
  .\scripts\fake-event.ps1 -Event PreToolUse -Tool Bash -Target "npm test"
  .\scripts\fake-event.ps1 -Event PermissionRequest -Tool Bash -Target "git push"
  .\scripts\fake-event.ps1 -Event Stop -Message "Added the login page"
  .\scripts\fake-event.ps1 -Event StopFailure
  .\scripts\fake-event.ps1 -Event SessionEnd
#>
param(
  [ValidateSet("SessionStart", "UserPromptSubmit", "PreToolUse", "PostToolUse", "PostToolUseFailure",
    "Notification", "PermissionRequest", "Stop", "StopFailure", "SessionEnd")]
  [string]$Event = "PreToolUse",
  [string]$Tool = "Edit",
  [string]$Target = "src\main.rs",
  [string]$Message = "All done! I added the feature and the tests pass.",
  [string]$Session = "fake-session-1",
  [string]$Cwd = (Get-Location).Path
)

# Windows PowerShell 5.1 pipes text to programs as ASCII by default; use UTF-8.
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)

$candidates = @(
  (Join-Path $env:LOCALAPPDATA "Glowby\bin\glowby-hook.exe"),
  (Join-Path $PSScriptRoot "..\target\release\glowby-hook.exe"),
  (Join-Path $PSScriptRoot "..\target\debug\glowby-hook.exe")
)
$hook = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $hook) { Write-Error "glowby-hook.exe not found. Run: npm run hook"; exit 1 }

$toolInput = switch ($Tool) {
  { $_ -in "Bash", "PowerShell" } { @{ command = $Target } }
  "WebFetch" { @{ url = $Target } }
  default { @{ file_path = $Target; new_string = "// changed by fake-event.ps1" } }
}
$payload = [ordered]@{
  session_id             = $Session
  cwd                    = $Cwd
  hook_event_name        = $Event
  tool_name              = $Tool
  tool_input             = $toolInput
  tool_use_id            = "toolu_fake"
  notification_type      = "permission_prompt"
  message                = $Message
  last_assistant_message = $Message
  error_type             = "rate_limit"
}
$json = $payload | ConvertTo-Json -Compress -Depth 5

$watch = [Diagnostics.Stopwatch]::StartNew()
$output = $json | & $hook $Event
$watch.Stop()
"exit code: $LASTEXITCODE    took: $($watch.ElapsedMilliseconds) ms"
if ($output) { "hook printed: $output" } else { "hook printed nothing (= no opinion, Claude Code continues normally)" }
