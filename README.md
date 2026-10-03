# Glowby

**Made by [Shadow-PJ](https://github.com/Shadow-PJ).**

[![Latest release](https://img.shields.io/github/v/release/Shadow-PJ/companion-?label=download)](https://github.com/Shadow-PJ/companion-/releases/latest)
[![CI](https://github.com/Shadow-PJ/companion-/actions/workflows/ci.yml/badge.svg)](https://github.com/Shadow-PJ/companion-/actions/workflows/ci.yml)
![Windows 10/11](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D6)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

A small glowing jellyfish that lives at the top edge of your screen and keeps you
company while [Claude Code](https://claude.com/claude-code) and Codex work. It shows what
your agents are doing, lets you answer Claude's permission questions with one click, chats
with Claude for you, and levels up as you code.

Glowby hides at the top edge and slides out when you hover there. He's drawn entirely
in code (no image files), uses about 60 MB of RAM and ~0% CPU while hidden, and never
sends your data anywhere.

## Download

1. Download **[Glowby.exe](https://github.com/Shadow-PJ/companion-/releases/latest/download/Glowby.exe)**
   from the [latest release](https://github.com/Shadow-PJ/companion-/releases/latest).
   It's one file; there's nothing to install.
2. Put it anywhere you like (your Desktop, `C:\Tools` …) and double-click it.
3. If Windows says **"Windows protected your PC"**, click **More info → Run anyway**.
   Glowby isn't code-signed (that costs money every year), so Windows doesn't know it yet.

**You need:**

* Windows 10 or 11 (64-bit)
* The Microsoft Edge **WebView2** runtime: already part of Windows 11 and most Windows 10
  PCs. If Glowby doesn't open, [get it from Microsoft](https://developer.microsoft.com/microsoft-edge/webview2/).
* [Claude Code](https://claude.com/claude-code), installed and signed in

**Start Glowby with Windows (optional):** press `Win + R`, type `shell:startup`, press
Enter, and put a shortcut to `Glowby.exe` in the folder that opens.

## First run

1. Glowby opens **Settings**. Click **Connect Claude Code + Codex…** and review both
   changes. Click **Apply both connections**; Glowby backs up each settings file first.
2. Restart your Claude Code and Codex sessions. In Codex, enter `/hooks` and trust
   Glowby's hook entries.
3. Hover the top edge of your screen (the thin line) to call Glowby out.

Codex hooks only report status in the background. They can never approve, deny, delay, or
block a Codex action. If Glowby is closed, Codex keeps working normally.

## What Glowby does

**Watches Claude Code for you**

* **Live status:** hover the line at the top edge to see what Claude is doing right now
  ("Editing main.rs", "Running npm test"). The line turns blue while Claude works and
  amber when it needs you.
* **Permission questions:** when Claude asks to run something, answer **Allow / Deny** on
  Glowby. No answer in time? The question goes back to the terminal.
* **Moods:** working, happy when a task is done, alert when Claude needs you, sick while
  your tests or build fail, sleepy when it's quiet.

**Helps you**

* **Chat:** click Glowby to ask Claude Code about your project.
* **Quick actions:** right-click Glowby: explain the last error, run and fix the project,
  commit with a good message … (editable in Settings).
* **Drop a file** on Glowby, then pick Explain / Review / Fix / Summarize.
* **Error watcher** (off by default): copy an error message and Glowby offers to fix it.
* **Daily briefing:** yesterday's commits, unfinished work and one small next step.
* **Learn mode:** after Claude changes your code, a quick question about it for XP.
* **Break reminder** after long coding stretches.
* **GitHub CI check** (off by default): tells you when GitHub Actions fails.

**Grows with you**

* **XP and levels** from finished tasks, fixes, passing tests and commits. Glowby
  evolves at levels 6, 15 and 30.
* **Anime-style pets:** switch Glowby for one of his friends: **Neko** (a cat),
  **Kitsune** (a fox spirit), **Slime**, **Mini Ninja** or **Mini Robot**. New ones
  unlock as you level up (Settings → Pets and characters).
* **They're alive:** pets wave hello when they slide out, look around, stretch, hop and
  wag their tails. Emotes: wave, jump, cheer, laugh, peace sign, shy, sparkle eyes,
  hearts, spin, dance and fireworks.
* **Petting:** stroke your pet with the mouse (move back and forth over him). He purrs,
  blushes and sends hearts, and it counts for a daily quest.
* **Unlockables:** hats, colours, emotes, and power-up **auras** (flames, golden
  power-up, cursed energy, infinity rings …).
* **Your own characters:** Settings → Characters → **Import a picture…** and Glowby wears
  any character you like (your favourite anime hero, your cat …) as a round icon, with
  all his moods. Pictures stay on your PC.
* **Daily quests and streaks.** Ignore him for days and he gets tired, but he never dies.
* **Squad mode** (off by default): one small pet for each running Claude Code session,
  each levelling up on its own. Click one to see what it's doing or chat with it.

Every feature has its own switch in Settings. **Game mode** hides Glowby and mutes him
while a fullscreen game runs.

## Privacy and safety

* **No telemetry.** Everything stays on your PC: settings and progress in
  `%APPDATA%\dev.glowby.app`, the hook program in `%LOCALAPPDATA%\Glowby\bin`.
  Glowby itself only goes online for the optional GitHub CI check; chat and learn mode
  go through Claude Code with your own login.
* **Claude Code is never blocked.** The hooks "fail open": if Glowby is closed or
  crashes, Claude Code works exactly as if Glowby didn't exist.
* **Your Claude settings are changed only after you see the diff**, with a backup first.
  **Uninstall hooks** removes only Glowby's entries.
* A GitHub token (only if you use the CI check) is kept in Windows Credential Manager,
  never in a file.
* The connection between Claude Code and Glowby is locked to your Windows account.

## Uninstall

1. Settings → **Uninstall hooks…** → **Apply**.
2. Quit Glowby (tray icon → Quit) and delete `Glowby.exe`.
3. Optional, to remove your data too: delete `%APPDATA%\dev.glowby.app`,
   `%LOCALAPPDATA%\Glowby` and `%LOCALAPPDATA%\dev.glowby.app`.

## Troubleshooting

* **Glowby doesn't react to Claude Code:** open Settings and check that
  **Connect to Claude Code** says *Connected*, then restart your Claude Code sessions.
* **I can't find Glowby:** hover the very top edge of the screen where the faint line
  is, or use the tray icon → **Show Glowby**. Game mode hides him while a fullscreen app
  runs.
* **Started Glowby from inside the Claude desktop app's terminal?** The Store version of
  the Claude app redirects AppData for programs it starts, so Glowby's data ends up in a
  private folder. Double-click `scripts\move-out-of-claude-app.cmd` in File Explorer once
  to move it back, then start Glowby normally.
* `glowby.log` in `%APPDATA%\dev.glowby.app` records startup and errors. Start Glowby
  with `GLOWBY_DEBUG=1` to also log every hook event.

## Build it yourself

Needs Rust (MSVC toolchain), Node 22+, the Visual Studio C++ build tools and WebView2.

```powershell
git clone https://github.com/Shadow-PJ/companion-.git glowby
cd glowby
npm install
npm run release        # builds target\release\glowby.exe (the hook program is built in)
.\target\release\glowby.exe
```

* `npm run dev`: development mode with live reload of the web parts.
* `npm run vite:dev`, then open `http://127.0.0.1:1420/gallery.html`: every stage, mood,
  hat, colour, emote and aura side by side.
* `cargo test --workspace`: the Rust tests.
* `.\scripts\fake-event.ps1 -Event Stop -Message "Done!"`: send Glowby a pretend Claude
  Code event without running Claude Code.
* `.\scripts\measure.ps1 -Seconds 60`: measure RAM and CPU.

**Publishing a new version:** bump the version in `Cargo.toml`, `package.json` and
`src-tauri/tauri.conf.json`, then push a tag (`git tag v0.2.0` → `git push origin v0.2.0`).
GitHub Actions builds `Glowby.exe` and publishes the release.

```
crates/glowby-protocol   messages + security helpers shared by app and hook
crates/glowby-hook       the tiny program Claude Code runs (fail-open)
src-tauri/src            the app: pipe server, sessions, windows, chat, installer
src/pet                  the pet window (Canvas character, bubble, chat)
src/settings             the settings window
scripts/                 icon generator, fake events, measurements
```

[LEARNING.md](LEARNING.md) explains how every part works, phase by phase.

## Credits

Glowby was designed and made by **[Shadow-PJ](https://github.com/Shadow-PJ)**, built
with the help of Claude Code. The character, its name and its sounds are original.

## License

[MIT](LICENSE) © 2026 Shadow-PJ. You may use, copy and change Glowby, also in your own
projects, as long as you keep the copyright notice and license text. It comes "as is",
without warranty.
