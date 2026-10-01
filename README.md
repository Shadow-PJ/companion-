# Glowby

A small glowing jellyfish that lives at the top edge of your screen and keeps you
company while Claude Code works. Windows 10/11, built with Tauri 2 (Rust + TypeScript).
The character is drawn entirely in code (Canvas); there are no image files.

## Build and run

Requirements: Rust (MSVC toolchain), Node 22+, Visual Studio C++ build tools, Windows SDK, WebView2.

```powershell
npm install
npm run release            # builds target\release\glowby.exe + glowby-hook.exe
.\target\release\glowby.exe
```

For development with live reload of the web parts: `npm run dev`.

## First run

1. Glowby opens **Settings** because it isn't connected yet.
2. **Connect to Claude Code → Install hooks…** shows the exact diff for
   `~\.claude\settings.json`. Click **Apply**. A backup is saved first.
3. Restart any running Claude Code sessions.
4. Hover the top edge of your screen (where the thin strip is) to call Glowby out.

## Testing without Claude Code

`scripts\fake-event.ps1` runs the real `glowby-hook.exe` with a pretend event:

```powershell
.\scripts\fake-event.ps1 -Event UserPromptSubmit
.\scripts\fake-event.ps1 -Event PreToolUse -Tool Edit -Target src\main.rs
.\scripts\fake-event.ps1 -Event PermissionRequest -Tool Bash -Target "git push"
.\scripts\fake-event.ps1 -Event Stop -Message "Added the login page"
```

## Troubleshooting

* `glowby.log` in `%APPDATA%\dev.glowby.app` records startup and errors. Start Glowby
  with `GLOWBY_DEBUG=1` to also log every hook event it receives.
* Set `GLOWBY_HOOK_DEBUG=1` before running `glowby-hook.exe` by hand to see why an event
  wasn't delivered (not running, bad JSON, untrusted pipe owner).

## Measuring

```powershell
.\scripts\measure.ps1 -Seconds 60 -Label "hidden, idle"
```

Counts glowby.exe plus all of its WebView2 helper processes.

## Safety

* Hooks **fail open**. If Glowby is closed, crashed, or slow, the hook exits 0 and
  prints nothing, so Claude Code behaves as if Glowby didn't exist. Glowby never uses
  exit code 2 (Claude Code's "block" signal).
* Listening hooks are `async`, so Claude Code never waits for them. Only the permission
  hook waits, and only while Glowby is running and you haven't answered.
* `settings.json` is changed only after you review a diff. It's backed up first, written
  atomically, and verified. **Uninstall hooks** removes only Glowby's entries.
* No telemetry. Data lives in `%APPDATA%\dev.glowby.app` (settings, backups, chat
  session IDs) and `%LOCALAPPDATA%\Glowby\bin` (the hook program).
* The hook pipe is locked to your Windows account. The hook also checks that whoever
  is listening runs as you before it sends anything.

## Uninstall

Settings → **Uninstall hooks…** → Apply. Then delete `%APPDATA%\dev.glowby.app`,
`%LOCALAPPDATA%\Glowby` and `%LOCALAPPDATA%\dev.glowby.app`.

## Layout

```
crates/glowby-protocol   messages + security helpers shared by app and hook
crates/glowby-hook       the tiny program Claude Code runs (fail-open)
src-tauri/src            the app: pipe server, sessions, windows, chat, installer
src/pet                  the pet window (Canvas character, bubble, chat)
src/settings             the settings window
scripts/                 icon generator, fake events, measurements
```

See LEARNING.md for how each part works.
