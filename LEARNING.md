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
