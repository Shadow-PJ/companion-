# Learning notes

Plain-language explanations of how Glowby works, phase by phase.

## Phase 1: the core

### 1. Two programs, not one

Claude Code can run a command on every *hook event* (session start, before a tool runs,
a permission question, done…). It sends the event as JSON on the command's **stdin**.

If that command were Glowby itself, Claude Code would have to start a whole app with a
browser engine for every event. Instead there are two programs:

* `glowby-hook.exe`: tiny (a few hundred KB), starts in milliseconds, does one thing.
  It forwards the event to Glowby and, for permission questions, waits for the answer.
* `glowby.exe`: the app you see, always running in the background.

They talk over a **named pipe**, Windows' built-in local channel (`\\.\pipe\name`).

### 2. "Fail open"

A safety rule: if anything goes wrong, *let things continue as normal*.

* If Glowby isn't running, the pipe doesn't exist, so the hook gives up within 150 ms
  (usually instantly), prints nothing, and exits with code 0. To Claude Code that means
  "no opinion".
* If the hook crashes, `catch_unwind` turns the crash into a normal exit 0.
* Glowby never uses exit code 2, which is how a hook *blocks* an action.
* If you don't answer a permission question in time, Glowby replies "pass" and Claude
  Code shows its normal prompt in the terminal.

The opposite, "fail closed", is right for security gates (deny if unsure). For a
companion app, never getting in the way matters most.

### 3. Async vs waiting hooks

Most of Glowby's hooks are marked `"async": true` in settings.json. Claude Code starts
them and moves on without waiting, so they cost you zero delay. Only `PermissionRequest`
is synchronous, because Claude Code needs the answer.

### 4. Named pipe security

A pipe name is global on the PC, so we add two locks:

1. **Access control list (ACL)**: the pipe is created with the security descriptor
   `D:P(A;;GA;;;<your SID>)`, "only this Windows account may open it". A SID is the
   unique ID of a Windows account.
2. **Server check**: before sending anything, the hook asks Windows which process is
   listening (`GetNamedPipeServerProcessId`) and checks that it runs as the same user.
   If another account grabbed the pipe name first, the hook stays silent instead of
   trusting it. Otherwise a fake "Glowby" could approve commands for you.

### 5. Event-driven, not polling

*Polling* means asking "anything new?" on a timer, which wakes the CPU even when nothing
happens. Glowby waits for Windows to tell it things instead:

| Need | How Glowby gets told |
|---|---|
| Claude did something | the hook connects to the pipe |
| Mouse touched the top edge | `WM_MOUSEMOVE` message to the 3-pixel strip window |
| A game went fullscreen | `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` |
| "Happy" should fade to "idle" in 6 s | one timer set for exactly that moment |

The one exception: while Glowby is **visible**, it reads the cursor position about
30 times per second (for the eyes and click-through). That stops the instant it hides.

### 6. The window tricks (Win32 extended styles)

* `WS_EX_NOACTIVATE`: clicking Glowby doesn't take focus from your editor or game.
  Glowby only becomes focusable while you type in the chat box, then hands focus back.
* `WS_EX_TOOLWINDOW`: no taskbar button, not in Alt-Tab.
* `WS_EX_LAYERED | WS_EX_TRANSPARENT`: the mouse passes through the window. The page
  reports the rectangles where the pet and bubble are drawn, and Glowby switches
  click-through off only while the cursor is over those.
* Transparent window + transparent page background = only the jellyfish is visible.

Tauri's own window helpers rewrite all of these style bits whenever they change one,
so after creation Glowby manages this window with direct Win32 calls.

### 7. Drawing with Canvas

The jellyfish is about 40 drawing commands per frame: Bézier curves for the bell, a
radial gradient for the glow, sine waves for the tentacles.

* `requestAnimationFrame` runs the loop in sync with the screen. Glowby caps it at 60
  frames per second and **stops it completely** when hidden.
* Colours *blend* between moods: each frame moves the current colour a little toward the
  target colour (`1 - exp(-dt * 5)`), so the change takes the same time at any frame rate.
* High-DPI screens: the canvas has `devicePixelRatio` times more real pixels than its CSS
  size, so lines stay sharp.

### 8. Editing settings.json safely

1. **Preview**: compute the new file in memory and show a line diff (the `similar` crate).
2. **Token**: the preview includes a hash of the file. If the file changed before you
   clicked Apply (for example Claude Code edited it), Apply refuses.
3. **Backup**: copy the old file to `%APPDATA%\dev.glowby.app\backups\`.
4. **Atomic write**: write `settings.json.tmp`, then rename it over the original. A
   rename is all-or-nothing, so a crash can't leave a half-written file.
5. **Verify**: read it back and compare.

Glowby's entries are recognised by their command path, so uninstall removes only those.

### 9. The chat box and command injection

The chat runs `claude -p` (print mode) in your project folder. Your message is written to
claude's **stdin**, not passed as a command-line argument. On Windows, command lines go
through quoting rules that differ per program, and `cmd.exe` treats `& | ^ "` specially.
Text from a user should never be glued into a command line. Glowby also starts
`claude.exe` directly instead of the `claude.cmd` shim, so `cmd.exe` is never involved.

`--output-format stream-json` makes claude print one JSON object per line as it works,
so Glowby can show "Reading main.rs…" while you wait.

### 10. Measuring a WebView app honestly

WebView2 is Microsoft Edge's engine. It runs as several processes (browser, GPU,
renderer, utility). Task Manager groups them under "Glowby", and `measure.ps1` adds them
all up. The "Memory" column is the **private working set**: RAM used by these processes
alone, not counting shared system libraries.

Two Chromium switches saved about 11 MB by merging helper processes into the main one:
`--in-process-gpu` and `--enable-features=NetworkServiceInProcess2`. We tried them first
through the `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` environment variable (no rebuild
needed), then checked on screen that transparency still worked before keeping them.

### 11. Bugs we hit while building Phase 1 (and what they teach)

* **The invisible BOM.** Windows PowerShell 5.1 can put a byte-order mark (`EF BB BF`)
  in front of text it pipes into a program. `serde_json` rejects it, so the hook (by
  design) silently gave up, and test events from PowerShell never arrived while real
  Claude Code events did. Fix: strip a leading BOM. Lesson: "fail silently" needs a debug
  switch (`GLOWBY_HOOK_DEBUG=1`), or you can't tell *why* it failed.
* **Screenshots missed the status line.** Plain screen copies skip *layered* windows.
  Copying with the `CAPTUREBLT` flag includes them.
* **Settings popped up behind a game.** First-run Settings now waits until the
  fullscreen app closes.
* **Tauri rewrites window styles.** Its window helpers recompute *all* extended styles
  from their own flags, which would drop `WS_EX_NOACTIVATE`. We read Tauri's (`tao`'s)
  source to confirm, then avoided those helpers after creating the window.
* **You couldn't type in the chat** (found by you while testing). Two causes stacked up:
  1. Tauri's `set_focus()` first checks its *own* "is the window visible?" flag. Glowby
     shows the window with a direct Win32 call, so Tauri thought it was hidden and did
     nothing.
  2. Windows' *foreground lock*: a program may only bring itself to the front if it
     received the user's last input. Your click went to WebView2's helper process
     (`msedgewebview2.exe`), not to `glowby.exe`, so `SetForegroundWindow` was refused.

  Fix: while the chat is open, Glowby drops `WS_EX_NOACTIVATE` (so any click inside it
  activates the window the normal way), forces the foreground with the same Alt-key
  fallback Tauri uses internally, and focuses the web page directly. Lesson: when you
  bypass a framework for one thing, check which of its other features relied on the
  state you bypassed.
* **…and then the chat closed itself instantly.** Diagnostics (`GLOWBY_DEBUG=1` adds
  page events to `glowby.log`) showed the real story: the window *did* activate, but
  while Windows hands focus from the window frame to the web page inside it, the frame
  reports "lost focus" for about 1 ms. The "close chat when you click away" rule reacted
  to that blip, so the chat opened and closed six times in two seconds while you clicked.
  Fix: only treat it as "clicked away" if another program is *still* in front 600 ms
  later. Two lessons:
  1. Measure before fixing. Two rounds of guessing produced two plausible fixes for
     the wrong problem; one round of logging found the real one.
  2. Focus events are noisy. Never act on a single focus-lost event; confirm it.
* **The Alt-key trick was removed.** Faking an Alt press to win the foreground can put
  the window into Windows' "menu mode", which swallows typing. Glowby uses
  `AttachThreadInput` as the fallback instead (here plain `SetForegroundWindow` already
  worked once the window wasn't marked no-activate).
* **The status line was invisible when idle**, so you had no idea where to hover. It's
  now always faintly visible and bright only when something happens. A UI hint that only
  appears when it's least needed isn't a hint.
* **The chat box was hidden until a folder was set.** Now it uses your latest Claude
  Code project by default, and the folder chip in the bubble opens a folder picker.

## Phase 2: helpful actions

### 1. Quick actions are prompt templates

A quick action is just a label plus a prompt with placeholders: `{last_error}`,
`{project}`, `{today}`. Glowby fills them in and sends the result through the same chat
pipeline as your typed messages. "Last error" is whatever Glowby noticed most recently:
a failed command from a Claude session, failing tests, or an error you copied. It is
kept in memory only.

Read-only actions (Explain, What did I change today?) are enforced, not just requested:
while one runs, the chat's PreToolUse gate *denies* Edit/Write tools. Plain read
commands like `git log` or `git diff --stat` are allowed without asking. Anything with
`;`, `&`, `|`, `>`, `<`, `` ` `` or `$` is never auto-allowed, because those characters
let one "harmless" command smuggle in another.

### 2. Drag and drop on Windows uses COM

While you drag a file, the drag owns the mouse ("capture"), so other windows get no
mouse messages and the hover strip can't notice you. Windows' drag and drop goes
through **OLE**: windows register a *drop target* (a COM object implementing
`IDropTarget`), and Windows calls its `DragEnter`, `DragOver`, `DragLeave` and `Drop`
methods. Glowby's strip registers one that accepts nothing (`DROPEFFECT_NONE`) but
summons Glowby in `DragEnter`. The actual drop then lands on the pet window, where Tauri
turns it into a `DragDrop` event with the file paths. Files outside the project are
made readable for Claude with `claude --add-dir <folder>`.

COM in Rust: the `windows` crate's `#[implement(IDropTarget)]` macro generates the COM
plumbing (reference counting, interface tables); we just write the four methods.

### 3. The clipboard watcher: event-driven and private

`AddClipboardFormatListener` asks Windows to send `WM_CLIPBOARDUPDATE` to a window on
every clipboard change. No polling, and nothing at all happens while the setting is off,
because the listener isn't registered. On a change Glowby:

1. waits 150 ms (the copying app may still be writing),
2. skips the clipboard entirely if it carries a "private" marker that password managers
   set (`ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`),
3. reads the text (max 20,000 characters) and runs a **rule-based classifier**: strong
   signals (`Traceback (most recent call last)`, `panicked at`, `npm ERR!`, …) or at least
   two weaker ones (an `…Error:` line, `error TS1234`, a stack-frame line like
   `at App (src/App.tsx:12:20)`, …),
4. forgets the text immediately unless it's an error.

Why rules and not an AI model? It runs instantly, offline, costs nothing, and you can
read exactly what it does. The tests list errors it must catch and normal text it must
ignore.

### 4. "Sick until tests pass" from hook events

We captured real hook payloads first instead of trusting the docs:

* success: `PostToolUse` with `tool_response: {stdout, stderr, interrupted}`
* failure: `PostToolUseFailure` with `error: "Exit code 1 …"` and `is_interrupt`

Glowby classifies the command (`cargo test`, `npm run build`, `pytest`, …) by matching
whole words, so `python makedirs.py` isn't mistaken for `make`. Failures are stored per
project in `health.json`, so the sick state survives a restart. A passing run of the same
kind clears it, and passing tests also clear a build failure (they had to build first).

### 5. The bug that taught the most: waiting for end-of-file

Testing Phase 2 revealed 49 `glowby-hook.exe` processes that never exited. The hook read
stdin *until end-of-file*, but for async hooks Claude Code sometimes never closes stdin,
so those hooks waited forever. (Claude Code wasn't affected, because async hooks don't
block it, but the processes piled up.)

Fixes:

* read exactly **one JSON value** and stop at its closing brace (`serde_json`'s stream
  deserializer), so the hook never needs end-of-file;
* a **watchdog** thread: whatever happens, a hook ends itself after 15 s (or the
  maximum permission wait plus 15 s);
* a test whose input *panics* if anyone tries to read past the object.

Lesson: when you design a protocol, decide how a message *ends*. "When the stream
closes" depends on the other side behaving perfectly. "When the JSON object closes" is
under your control.

### 6. Updating a program that is running

Windows won't overwrite or delete an `.exe` while it runs, and Claude Code starts the
hook on every tool call. But Windows **does** allow *renaming* a running program. So the
updater moves the old copy aside (`glowby-hook.old-….exe`), puts the new one in place,
and deletes old copies on a later start.

## Phase 3: progression

### 1. An XP economy you can't farm

Glowby rewards real work: a finished task that used tools (10 XP), fixing failing tests
or builds (30), a passing test run (5), a commit (8), taking a break (5), a daily streak
bonus (5 × streak, capped at 35). A chat-only turn gives only 2. Passing tests give XP
at most once per 10 minutes per project, so re-running a green test suite in a loop
earns nothing. When you design a reward system, ask "what's the cheapest way to game
this?" and close that door.

### 2. A level curve from one formula

Reaching level L needs `25 · L · (L − 1)` XP in total: 0, 50, 150, 300, 500 … Each level
costs 50 more than the last (an arithmetic series), so early levels come fast and later
ones take steady work. `level_for(xp)` just counts up until the next threshold is out of
reach.

### 3. Evolution and cosmetics are data plus drawing code

All unlockables live in one table (`COSMETICS` in `progress.rs`): id, kind, name, and a
requirement (`Level(n)` or `Streak(n)`). Unlocking is computed from your level and best
streak; nothing extra is stored. Settings, the right-click menu and the level-up message
all read the same table, so they can't disagree.

The four stages are layers added on top of the same jellyfish: Lantern adds a radial
gradient "light" and rim dots, Starlit adds glowing spots and orbiting sparkles, Aurora
rotates the colours around the colour wheel (`hueShift`, a standard RGB rotation matrix)
over time. Hats are small drawings placed at the top of the bell, inside the same
transform, so they bob, tilt and scale with Glowby.

### 4. Time-based state without timers

Energy ("ignored for days") isn't stored as a number that a timer decreases. Glowby
stores *when you were last seen* and computes energy when it's needed:
`100 − 30 × days away`, never below 10. The same idea gives the streak (compare today's
date with the last coding day) and the break reminder (session start + interval). The
only timer is the one that already existed: it sleeps until the earliest "something
changes at this moment" deadline. Storing timestamps instead of counters is a classic
way to avoid background work, and it survives restarts for free.

### 5. Sounds from oscillators

WebAudio can make sound from scratch: an `OscillatorNode` produces a wave (sine,
triangle …) at a frequency, and a `GainNode` shapes its volume over time (an
*envelope*: quick fade-in, smooth fade-out). Each Glowby sound is two to six such notes,
for example C-E-G-C for "level up". No files, no licences, any pitch you like.

Two Windows/Chromium details:

* Browsers block sound until the user clicks the page (*autoplay policy*). Glowby's
  WebView2 is started with `--autoplay-policy=no-user-gesture-required` so event sounds
  can play.
* A running `AudioContext` keeps an audio stream open, which costs CPU even in silence.
  Glowby suspends it one second after each sound; we measured that CPU drops back to
  0.01%.

### 6. Testing visuals and state safely

* `gallery.html` (dev only, `npm run vite:dev`, then open `/gallery.html`) draws every
  stage, mood, hat, colour and emote side by side, so you can check the art without
  playing the game for weeks.
* To test level-ups in the real app, we backed up `progress.json`, sent fake hook events
  worth exactly 50 XP (level 2), checked the screen, then restored the backup. Test with
  real data paths, but leave the user's data exactly as you found it.

## Phase 4: smart features

### 1. The daily briefing is plain git, run once a day

Glowby remembers which git repositories your Claude sessions ran in (`projects.json`:
on the first event from a new folder it runs `git rev-parse --show-toplevel` once). On
your first activity of the day it asks git:

| Question | git command |
|---|---|
| What did I do yesterday? | `git log --since=… --until=… --author=<you> --pretty=format:%s` |
| Uncommitted work? | `git status --porcelain` (one line per changed file) |
| Unpushed commits? | `git rev-list --count @{u}..HEAD` (`@{u}` = the upstream branch) |
| A TODO nearby? | `git grep -n -E "TODO|FIXME" -- <files in the last commit>` |

The "one small next step" is a priority list (failing checks, commit, push, TODO,
"write a test") rather than an AI call: instant, free, predictable, and easy to test.
Git always runs without a shell, with no console window, and with a timeout.

### 2. Learn mode: an LLM call with the doors closed

Questions are written by Claude's small model:
`claude -p --model haiku --tools "" --no-session-persistence --settings {"disableAllHooks":true}`.

* `--tools ""`: the model can't run commands or edit files; it only writes text.
* `disableAllHooks`: otherwise the quiz request would itself fire Glowby's hooks and
  count as one of your coding sessions (we checked the log to confirm it doesn't).
* It runs in Glowby's own data folder, so it reads no project files or CLAUDE.md.
* The model reply is untrusted text: Glowby extracts the JSON object, checks it (2–4
  options, valid answer index) and **shuffles the options**, because models tend to
  put the right answer first (ours did in the test).

### 3. Quests seeded by the date

The three daily quests are chosen by a tiny pseudo-random generator (an LCG) seeded with
a hash of the date. Same day, same quests, even after a restart; a new day gives a new
set. That's the same trick games use for "daily challenges". Progress comes from events
Glowby already sees: finished tasks, commits, fixes, passing tests, test files written,
coding minutes (time between your events, ignoring gaps over 5 minutes), learn-mode
answers and breaks.

### 4. Secrets belong in the OS credential store

The GitHub token goes into **Windows Credential Manager** (`CredWriteW` / `CredReadW`),
the same place Windows keeps saved network passwords. It's tied to your Windows account,
never written to a file Glowby controls, and never sent back to the page after you save
it. The test stores a throwaway secret under its own name, reads it back and deletes it,
so it never touches the real token.

### 5. Being polite on someone's screen

Phase 4 added things that wait for you (a briefing, a question, a failed CI run). If
each one kept Glowby slid out until answered, he could cover the top of your screen for
hours. Now new items keep him out for about a minute (`hold_out`), then he hides as
usual. The item stays waiting for you; hover to see it.

## Phase 5: squad mode and your own characters

### 1. One pet per session: the hooks already say which session

Every hook event carries a `session_id`. Phase 1 used it to track "what is each session
doing"; squad mode gives each live session its own little pet. Each pet's mood comes
from its own session only (needs you → alert, working → working, just finished → happy),
while Glowby keeps showing the most urgent mood of all of them.

* **Names that never change:** a session's name and colour come from a tiny hash of its
  id (FNV-1a), so "Pip" stays "Pip", even after a restart.
* **Separate levels:** each pet has its own XP in `squad.json` (1 per tool use, 10 per
  finished task, 8 per commit, 20 when failing tests pass again). The curve is steeper
  than Glowby's, because one session is short. `claude --resume` brings the same pet back;
  pets unused for 30 days are forgotten.
* **Squad mode off = zero work:** no squad state is kept and nothing extra is drawn.

### 2. Chatting with a session without disturbing it

You can't type into a Claude Code session running in someone else's terminal, and you
shouldn't. Instead Glowby runs
`claude -p --resume <session-id> --fork-session`: Claude loads that conversation, so it
knows what the session did, but **writes the reply to a new copy** with a new id. We
tested it on a throwaway session first: told it "remember PINEAPPLE", then asked a fork
from Glowby. It answered PINEAPPLE, and the original session was untouched. Follow-up
questions continue the copy; "New chat" throws the copy away.

### 3. Importing a picture safely

* **Decoding happens in the browser engine.** WebView2 already contains well-tested,
  sandboxed image decoders, so Glowby didn't add an image library to Rust. Settings
  draws the picture on a `<canvas>`; you drag and zoom; the circle is exported as a
  256×256 PNG.
* **Raw bytes, not JSON.** Tauri can send binary data both ways (`tauri::ipc::Response`
  and a raw request body). That beats JSON arrays of numbers (about 4× bigger) or base64
  (about 1.33× bigger plus decode work). The name travels in a header, percent-encoded,
  because headers must be ASCII ("Test Hero ✨" survives the trip).
* **Trust the content, not the name.** Rust checks the first bytes ("magic numbers": PNG
  starts with `89 50 4E 47`, JPEG with `FF D8 FF`), so a renamed `.txt` or a private key
  is refused. The saved icon must be a real PNG of at most 1024×1024.
* **Ids can't escape the folder.** Character ids are generated by Glowby and may only
  contain letters, digits and `-`, so a request for `..\..\settings` is refused (that's
  a *path traversal* attack).

### 4. Many pets, one animation loop

The squad is drawn on one wide canvas in the **same** `requestAnimationFrame` callback
as Glowby (the renderer has an `onFrame` hook), so there's still exactly one loop, and
it still stops completely while Glowby is hidden. Each mini pet reuses the full-size
drawing code inside `ctx.scale(0.5, 0.5)`. To make that possible, the jellyfish code was
split into shared pieces (`bodyMotion`, `drawMoodExtras` …) that the round character icon
also uses, so every look gets the same body language.

### 5. Procedural effects: auras

The auras (flames, golden power-up, cursed energy, infinity rings, petals …) are a few
dozen canvas shapes each, moved by `sin()` of time. Lightning must look random but not
jitter every frame, so it uses a *seeded* noise function (`noise(frameNumber)`) that only
changes 8 times a second. Some parts are drawn behind the body, some in front (petals
falling past the icon).

### 6. Testing a desktop app's web pages from a script

For the live test, Glowby was started with
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=… --remote-debugging-address=127.0.0.1`,
which opens Chromium's DevTools protocol on this PC only, for that one run. A 30-line
Node script then ran test steps *inside* Glowby's page (import a picture, open a squad
pet's card, chat with it) without clicking on the screen. A normal start never has
that port open.

## Phase 6: anime pets, emotes and petting

### 1. Characters made of parts

Each anime pet is drawn from a few simple shapes: a big circle for the head, a small
ellipse for the body, two "arms" (thick round-capped lines with a circle for the paw),
ears, a tail. "Chibi" proportions (a head as big as the body) are what make them look
anime-cute. Different pets reuse the same parts with different colours and extras
(stripes, a muzzle, spiky hair, a robot visor).

### 2. Poses are just numbers

Every arm has one angle: 0 = hanging down, π (≈3.14) = straight up. A pose is a pair of
angles: waving = right arm at ~2.25 swinging ±0.4, cheering = both at ~2.5, shy = both
at 2.75 (paws on the cheeks). Emotes ease *into* the pose and back out using
`sin(progress × π)`, so nothing snaps. Arms below the shoulder are drawn behind the head,
raised arms in front of it, so a waving paw is never hidden.

### 3. Expressions

A face is chosen in two layers: the mood gives the default (focused while working,
spirals while sick …), and a short-lived *expression* can override it: `> <` while
laughing, a wink with the peace sign, star eyes, `^ ^` with a blush while being petted.
The same expression names drive three different faces (the jellyfish, the anime eyes,
the robot's LED visor).

### 4. Idle life without timers

While Glowby is visible the animation loop already runs 60 times a second, so idle
actions cost nothing extra: every 10–24 seconds the loop picks a small action (look
around, stretch, hop, wag, wave). When Glowby is hidden the loop is stopped, so there
are no idle actions and no CPU use.

### 5. Recognising petting

Petting is "moving the mouse back and forth over the pet without pressing a button".
The detector (`src/pet/petting.ts`) counts *direction changes* of the mouse: two changes
within 1.4 s, each after at least 6 px of travel, count as petting. That ignores a mouse
just passing by, tiny hand jitter, slow movement and dragging. It was checked with
simulated mouse movements before it ever ran on screen. Rust gives at most one small XP
reward every 5 minutes, so petting stays fun rather than an XP farm.

## Version 0.3: auto-allow

### 1. Saying "yes" for you, with a safety net

Claude Code asks before risky steps, and Glowby's permission hook can answer `allow`.
Auto-allow does that automatically, either for a while (a deadline kept in memory, so a
restart always turns it off) or until you switch it off (full auto). Before allowing,
every question goes through `decide()` in `autoallow.rs`:

* **Never-auto list:** the command is split into lower-case *words* (`git push --force`
  → `git`, `push`, `--force`), paths keep only their last part (`/bin/rm` → `rm`), and a
  rule matches when its words appear one after another. That's why `npm run format` or a
  folder named `rm-old` don't trigger the `rm` rule, while `cd x && rm -rf build` does,
  and so does an `rm` hidden inside `bash -c "…"`.
* **Project folder:** file edits must stay inside the session's folder. A similar name
  (`C:\code\application` next to `C:\code\app`) or a `..\` escape still asks.
* **When in doubt it asks.** A false alarm costs one click; a wrong "allow" could cost
  your files.

### 2. Tested end to end

Unit tests cover the rules: everyday work is allowed; deleting, pushing, installing,
downloading, hidden encoded commands and outside-the-project edits still ask. Then the
real `glowby-hook.exe` sent a pretend "npm test" permission question to a running Glowby
with timed auto-allow on. It answered `allow` in about 0.1 s, the counter and the quiet
log showed it, and Stop turned it off.

### 3. Codex speaks the same language

Codex's hooks were added as "watch only" at first. Its documentation shows a
`PermissionRequest` hook whose answer looks exactly like Claude Code's
(`hookSpecificOutput.decision.behavior: "allow"`), so the same `glowby-hook.exe` can
answer both. Glowby tells them apart by the payload: Codex events carry a `turn_id`
and keep their logs under `~/.codex`. If Glowby doesn't answer, both tools fall back to
their normal prompt, so the "fail open" rule still holds.

### 4. The AI limit forecast: exact numbers, honest guesses

* **Codex** writes its real limit numbers (percent used, window length, reset time) into
  its session logs after every reply. Glowby reads only the end of the newest log.
* **Claude Code** hands its *status line* the exact 5-hour and weekly percentages. So
  Glowby offers to be the status line (only if you don't have your own), prints a short
  line, and passes the numbers on. Where no status line runs, Glowby adds up the token
  counts in your local transcripts for the current 5-hour window, and learns your limit
  the first time Claude stops with a rate-limit error.
* **The forecast** is a straight line through the last 90 minutes of readings: if you
  went from 40% to 60% in 40 minutes, 40% more takes about 80 minutes. That's why every
  number shows "as of …" and estimates carry a "≈": people speed up and slow down.

## Version 0.4: the token detective

### 1. Let the data decide

The first idea was "the cache expires after 5 minutes". The logs disagreed: every reply
records whether it wrote a 5-minute or a 1-hour cache, and this user's sessions used the
1-hour one. The report also prints the rebuild rate per pause length. Here it was 0% for
pauses under an hour and 100% above, so the real lifetime is visible in the data
instead of assumed.

### 2. Don't blame the user for normal behaviour

A session's first reply, the reply after `/compact`, and a model switch always rebuild
the cache, so they aren't counted. Neither are rebuilds without a break (Claude Code
changed its tools or settings). A file read again after Claude edited it, or after
`/compact`, isn't waste either: Claude needs the fresh copy. Partial reads (`offset`)
never count. Tips people stop trusting are worse than no tips.

### 3. A tolerant parser for a format that isn't an API

`crates/glowby-detective` reads the `.jsonl` logs line by line, skips anything it doesn't
recognise, and counts each reply once (one reply is written as several lines with the
same id). It never keeps message text; for a file read it keeps only the length.

### 4. Check the checker

The analyser started as a terminal command (`detective-report`) and its numbers were
compared with a second, independently written script before Glowby used them: same
reply count, same 8 rebuilds, same 3.4M tokens. Only then did the pet get its hat.
