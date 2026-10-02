# Glowby

**Made by [Shadow-PJ](https://github.com/Shadow-PJ).**

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

## Using Glowby

* **Hover** the faint line at the top edge: Glowby slides out with the live status.
* **Click** Glowby to chat with Claude Code in your project. The folder chip picks the project.
* **Right-click** Glowby for quick actions (edit them in Settings → Quick actions).
* **Drag a file** to the top edge, drop it on Glowby, then pick Explain / Review / Fix /
  Summarize or type an instruction.
* **Error watcher** (Settings → Helpers, off by default): copy an error message and Glowby
  offers to fix or explain it.
* **Sick?** Your tests or build are failing. Glowby gets better when they pass, or click
  Dismiss.
* **Levels:** Glowby earns XP from finished tasks, fixes, passing tests and commits, and
  evolves at levels 6, 15 and 30. Hats, colours and emotes unlock along the way
  (Settings → Glowby's progress). Keep a daily streak; ignore him for days and he gets
  tired (never worse).
* **Breaks:** after an hour of continuous coding (adjustable) Glowby suggests a rest.
* **Sounds:** small synthesized chimes. Volume, per-event toggles, and mute in Settings.
  Always muted during fullscreen games.
* **Daily briefing:** on your first activity of the day: yesterday's commits, unfinished
  work, and one small next step (Do it with Claude). Right-click → Today's briefing.
* **Learn mode:** after Claude edits your code, answer a quick question about it for XP
  (uses Claude Haiku through your login, at most every 30 minutes by default).
* **Daily quests:** 3 small goals a day with XP. Difficulty and types in Settings.
* **GitHub CI check** (off by default): Glowby tells you when Actions fails on your
  branch. The token is stored in Windows Credential Manager.
* **Your own characters:** Settings → Characters → **Import a picture…**, crop it to a
  circle, and Glowby wears it as a small round icon with all his moods, hats and emotes.
  Use pictures you have, e.g. your favourite anime characters. They stay on your PC.
* **Auras:** power-up effects (flames, golden power-up, cursed energy, infinity rings …)
  that unlock as you level up. Settings → Glowby's progress → Aura.
* **Squad mode** (off by default, Settings → Squad mode): one small pet per running
  Claude Code session, each levelling up on its own. Click a pet for what it's doing,
  its look, or **Chat with …** (talks to a copy of that session, so the original in your
  terminal isn't disturbed).

For art changes, `npm run vite:dev` and open `http://127.0.0.1:1420/gallery.html` to see
every stage, mood, hat, colour, emote and aura at once, plus a stand-in imported character.

## Testing without Claude Code

`scripts\fake-event.ps1` runs the real `glowby-hook.exe` with a pretend event:

```powershell
.\scripts\fake-event.ps1 -Event UserPromptSubmit
.\scripts\fake-event.ps1 -Event PreToolUse -Tool Edit -Target src\main.rs
.\scripts\fake-event.ps1 -Event PermissionRequest -Tool Bash -Target "git push"
.\scripts\fake-event.ps1 -Event Stop -Message "Added the login page"
```

## Troubleshooting

* **"Needs update" / Glowby gets no events, and you started Glowby from inside the Claude
  desktop app** (its terminal, or a Claude Code session in it): the Store version of
  the Claude app redirects AppData writes of programs it starts into its own private
  folder, so Glowby's hook program and data land where a normally started Glowby can't
  see them. Double-click `scripts\move-out-of-claude-app.cmd` in File Explorer once: it
  backs up, moves the data to the normal place, and restarts Glowby. Since this fix,
  `glowby.exe` carries its hook program inside, so any copy of it (e.g. on your Desktop)
  installs the hook by itself.

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
  session IDs, test/build health, progress, quests, recent projects, squad pets, your
  imported character pictures) and
  `%LOCALAPPDATA%\Glowby\bin` (the hook program). The only network traffic Glowby itself
  makes is the optional GitHub CI check (api.github.com, only when turned on); chat and
  learn mode go through Claude Code with your login.
* Every hook process ends itself within 15 s (permission questions: their timeout plus
  15 s), even if Claude Code never closes its input.
* The error watcher is opt-in, checks text locally, forgets non-errors immediately,
  never logs clipboard content, and skips content password managers mark as private.
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
