# Glowby

**Made by [Shadow-PJ](https://github.com/Shadow-PJ).**

[![Latest release](https://img.shields.io/github/v/release/Shadow-PJ/glowby?label=download)](https://github.com/Shadow-PJ/glowby/releases/latest)
[![CI](https://github.com/Shadow-PJ/glowby/actions/workflows/ci.yml/badge.svg)](https://github.com/Shadow-PJ/glowby/actions/workflows/ci.yml)
![Windows 10/11](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D6)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

A small glowing jellyfish that lives at the top edge of your screen and keeps you
company while [Claude Code](https://claude.com/claude-code) and Codex work. It shows what
your agents are doing, lets you answer their permission questions with one click, stops
messages that would waste your AI usage, tells you the moment a limit hits (and when it's
back), and levels up as you code.

Glowby hides at the top edge and slides out when you hover there. He's drawn entirely
in code (no image files). The v0.6.1 local release measured 64.1 MB of RAM and 0.002%
average CPU in a 45-second hidden sample, including WebView2 helpers, with the hub closed. Companion
data stays on your PC. Chat uses your agent account; AI Pulse reads public
news sources when you open it or opt into background checks.

> **Early version (v0.6.1).** Glowby is new and improving every week, so expect rough edges.
> Ideas and bug reports are very welcome: [open an issue](https://github.com/Shadow-PJ/glowby/issues).

## A calmer workspace in v0.6.1

* **One hub:** Settings opens inside AI Pulse. Categories and “Find a setting” replace
  the long scrolling page; individual hook controls stay available under Connections.
* **Both agents:** Home has Claude Code and Codex panels, individual chat buttons and
  usage readings. Chat's provider picker can pin either agent or return to Auto.
* **Better Claude news:** featured Anthropic announcements are read as well as normal
  newsroom entries. The briefing includes at most one patch release per CLI source.
* **Pet studio and direct reactions:** Wave, Pet and Play beside the desktop pet,
  plus a short interactive preview in Settings → Pet studio. These reactions use no AI.
* **Less idle drawing:** visible ambient animation is capped at 24 fps; short reactions
  use up to 60 fps. Hidden pets stop entirely. Reduced motion is available, previews
  release their canvases and listeners, and imported images are freed when leaving a
  category. Settings and news share the same light/dark theme and webview.
* **Reliable saving:** a visible Saved/Saving indicator, with Retry for disk errors.

Source search is local retrieval, not a ChatGPT chat. Sending a message with **Ask Claude**
or **Ask Codex** uses the selected agent's existing account and quota. Exact limit
percentages only appear when the provider reports them; missing values stay unknown.

## Download

1. Download **[Glowby.exe](https://github.com/Shadow-PJ/glowby/releases/latest/download/Glowby.exe)**
   from the [latest release](https://github.com/Shadow-PJ/glowby/releases/latest).
   It's one file; there's nothing to install.
2. Put it anywhere you like (your Desktop, `C:\Tools` …) and double-click it.
3. If Windows says **"Windows protected your PC"**, click **More info → Run anyway**.
   Glowby isn't code-signed (that costs money every year), so Windows doesn't know it yet.

**You need:**

* Windows 10 or 11 (64-bit)
* The Microsoft Edge **WebView2** runtime: already part of Windows 11 and most Windows 10
  PCs. If Glowby doesn't open, [get it from Microsoft](https://developer.microsoft.com/microsoft-edge/webview2/).
* [Claude Code](https://claude.com/claude-code) or Codex, installed and signed in

**Start Glowby with Windows (optional):** press `Win + R`, type `shell:startup`, press
Enter, and put a shortcut to `Glowby.exe` in the folder that opens.

## First run

1. Glowby opens **Settings**. Click **Connect Claude Code + Codex…** and review both
   changes. Click **Apply both connections**; Glowby backs up each settings file first.
2. Restart your Claude Code and Codex sessions. In Codex, enter `/hooks` and trust
   Glowby's hook entries.
3. Hover the top edge of your screen (the thin line) to call Glowby out.

The activity hooks run in the background. Only the permission hook waits for your answer
on Glowby, and if Glowby is closed (or doesn't answer in time) Claude Code and Codex show
their normal prompt, so they're never blocked.

## What Glowby does

**Watches Claude Code and Codex for you**

* **Live status:** hover the line at the top edge to see what your agent is doing right now
  ("Editing main.rs", "Running npm test"). The line turns blue while it works and
  amber when it needs you.
* **Permission questions:** when Claude Code or Codex asks to run something, answer
  **Allow / Deny** on Glowby. No answer in time? The question goes back to them.
* **Auto-allow:** right-click Glowby → **Auto-allow 15 min / 30 min / 1 hour**, or turn on
  **Full auto** in Settings, and Glowby answers Claude Code's and Codex's questions with
  Allow for you. Risky things still ask: deleting files, `git push`, `reset --hard`,
  installing software, downloads, and file changes outside the project (your
  **never-auto list** in Settings). A quiet log in Settings lists everything that was
  auto-allowed. (A chat in Claude's own **Auto** mode never asks, so there's nothing
  to allow there; Glowby tells you when that's the case.)
* **AI limits:** right-click Glowby → **AI limits** shows how much of your Claude (5-hour
  and weekly) and Codex limits you've used, when they reset, and a rough "you may run low
  in …" guess. When a limit gets tight, Glowby jumps out with a free handoff note for the
  other agent. The moment Claude hits a limit, Glowby tells you the exact time it's back,
  and tells you again when it is. Numbers are read on your PC: Codex's from its own logs,
  Claude's from Claude Code's status line in terminal sessions. (The Claude app doesn't
  share percentages, and claude.ai chats use the same limit, so there Glowby shows a token
  count instead of guessing.)
* **Saves your usage automatically:** a big chat whose cache has gone cold re-sends the
  whole conversation with your next message. One such message can eat a 5-hour limit.
  Glowby **stops that message once** and puts a fresh-start note with your message on
  the clipboard: paste it into a new chat, or send again to go ahead anyway. He also pops
  out two minutes before a big chat goes cold, and offers a fresh start when a chat gets
  very long. The note is built on your PC from the chat log (no AI, no tokens).
* **Alerts you can't miss:** for limits and stopped messages Glowby jumps out with a
  sound, nudges you again if you didn't see it, and sends a Windows notification
  (Settings can turn that off).
* **Token detective:** once a week Glowby puts on his detective hat 🔍 and shows a case
  report of what ate your Claude limits: cache rebuilds after breaks (measured against
  your real cache lifetime), files Claude re-read without changes, and sessions that got
  expensive because they ran long. Each finding has a tip and its share of your week.
  The report reads only numbers, times, tool names and file paths from your local logs.
  `detective-report` prints the same report in a terminal.
* **Moods:** working, happy when a task is done, alert when Claude needs you, sick while
  your tests or build fail, sleepy when it's quiet.

**Helps you**

* **Automatic agent switching:** run Claude Code or Codex and Glowby follows the most
  recently active session. Chat labels, routing, project context and visible usage follow
  that agent. You can pin an agent in Settings → Chat. Conversations stay separate.
* **Chat:** click Glowby to ask the active agent about your project using its existing
  login. Codex Ask is read-only; choose Accept edits explicitly for workspace edits.
* **Visible limits:** usage and reset information appear beside the pet automatically.
  Warnings pop out at your configured threshold. Unknown percentages remain unknown.
* **Permission feedback:** Allow/Deny confirms that the answer was sent; expired or
  disconnected requests show an explanation instead of pretending to succeed.
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
* **Squad mode** (off by default): one small pet for each running Claude Code or Codex session,
  each levelling up on its own. Click one to see what it's doing or chat with it.

Every feature has its own switch in Settings. **Game mode** hides Glowby and mutes him
while a fullscreen game runs.

## AI Pulse — your personal AI desk

Open **AI Pulse** from the pet's right-click menu, the tray, or Settings.

* A clean Home with five selected updates, a tool to explore, model movement and a
  measured leaderboard. Light, dark and system themes; responsive navigation.
* News and a release timeline from OpenAI, Anthropic, Google, Meta, Hugging Face and
  the official Codex / Claude Code repositories. Preview CLI builds are labelled.
* **Explore AI:** 19 tools across coding, image, video, audio, study, research, data,
  writing, agents and presentations. Each has use cases, price/access notes and alternatives.
* Model comparisons with dated provider specifications, an API request calculator,
  and Artificial Analysis's measured Intelligence Index, speed and task cost.
  Benchmark task cost is different from API token pricing. There is no universal winner.
* **Ask anything about AI:** searches the local news cache, model reference and tool
  directory, with source links. It does not call an AI model or spend your usage.
  Try “What happened with OpenAI this week?” or “Free AI for making videos”.
* Local bookmarks, company/model/topic follows, watchlist updates, **Ctrl/Cmd K** for
  the command palette and **Alt 1–7** to navigate.

Opening the hub refreshes old public data. Background checks are **off by default**;
enable them in AI Pulse → Preferences for watchlist and daily briefing notices.
The default interval is six hours (adjustable from 1 to 24 hours). Game mode defers
background requests and pet notices. Closing the hub releases its webview.

Every item shows its date and original source. Failed sources keep their last data
and display an error. Model/tool API prices are a **dated reference shipped with the
app**; public benchmark measurements refresh independently. Free access may have
credits, limits or hardware costs; check the linked provider before spending money.

### Connecting both agents

Use **Connect Claude Code + Codex** in Settings, review both diffs, and apply.
Restart agent sessions. In Codex, use **`/hooks`** to review and trust the installed
hooks. Glowby does not bypass that trust. A local file-change watcher keeps limits
and basic activity current even when a desktop session does not emit hooks.
Claude percentages require Claude Code's status-line data; if unavailable, Glowby
shows a token count or a reported limit/reset instead of inventing a percentage.
Pace forecasts are rough and become stale when you stop working.

## Privacy and safety

* **No telemetry.** Everything stays on your PC: settings and progress in
  `%APPDATA%\dev.glowby.app`, the hook program in `%LOCALAPPDATA%\Glowby\bin`.
  AI Pulse reads fixed public news feeds and benchmark pages when used or when you
  enable background checks. Search terms, project files, follows and bookmarks are not
  uploaded. Optional CI checks contact GitHub. Chat uses your selected agent with
  your existing login; learn mode uses Claude Code.
* **Claude Code is never blocked.** The hooks "fail open": if Glowby is closed or
  crashes, Claude Code works exactly as if Glowby didn't exist. The only thing Glowby ever
  stops on purpose is a message to a big cold chat (once, with the reason shown, and you
  can turn it off). If Glowby doesn't answer within 3 seconds, the message goes through.
* The fresh-start note uses your recent requests and the last answer from the chat log,
  only on your PC and only onto your clipboard.
* **Your Claude and Codex settings are changed only after you see the diffs**, with a backup first.
  **Uninstall hooks** removes only Glowby's entries.
* A GitHub token (only if you use the CI check) is kept in Windows Credential Manager,
  never in a file.
* The connection between Claude Code and Glowby is locked to your Windows account.

## Uninstall

1. Settings → **Uninstall hooks…** → **Apply**.
2. Quit Glowby (tray icon → Quit) and delete `Glowby.exe`.
3. Optional, to remove your data too: delete `%APPDATA%\dev.glowby.app`,
   `%LOCALAPPDATA%\Glowby` and `%LOCALAPPDATA%\dev.glowby.app`, and remove Glowby's
   notification name: `reg delete HKCU\Software\Classes\AppUserModelId\Shadow-PJ.Glowby /f`.

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
git clone https://github.com/Shadow-PJ/glowby.git glowby
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
* `npm run test:pulse`: check local hub retrieval and comparisons.
* `.\scripts\measure.ps1 -Seconds 60`: measure RAM and CPU.

**Publishing a new version:** bump the version in `Cargo.toml`, `package.json` and
`src-tauri/tauri.conf.json`, then push a tag (`git tag v0.6.1` → `git push origin v0.6.1`).
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
