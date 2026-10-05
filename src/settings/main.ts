// Settings window. Every change saves automatically (no "Save" button).

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { setCharacterImage } from "../pet/character/images";
import { DEFAULT_APPEARANCE, PetRenderer } from "../pet/renderer";
import { playSound } from "../pet/sound";
import { button, compact, el } from "../shared/dom";
import type { AppInfo, AutoAllowEntry, CharacterInfo, ChatMode, CosmeticView, LimitsView, GithubStatus, HooksPreview, HooksStatus, Look, MonitorInfo, ProgressInfo, Settings } from "../shared/types";
import { crop, nameFromFile } from "./cropper";
import type { View as PulseView, Preferences as PulsePreferences } from "../pulse/types";

const app = document.getElementById("app")!;
let settings: Settings;
let saveTimer = 0;
/** Your imported characters. */
let characters: CharacterInfo[] = [];
/** Redraws the progress card's previews (after the look changes elsewhere). */
let refreshProgress = () => {};

/// Debounced auto-save. The page's `settings` object stays the source of truth
/// (the editors hold references into it), so we don't replace it with Rust's copy.
function save() {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => void invoke<Settings>("save_settings", { settings }), 250);
}

// ---------- small building blocks ----------

function section(title: string, intro?: string | null, ...body: (Node | null)[]) {
  return el("section", { class: "card" }, el("h2", { text: title }), intro ? el("p", { class: "intro", text: intro }) : null, ...body);
}

/** Every connection card's status refresher: after any change, all of them
 *  re-check, so no card keeps showing an old "Needs update". */
const connectionRefreshers: (() => Promise<void>)[] = [];
async function refreshConnections() {
  await Promise.all(connectionRefreshers.map((refresh) => refresh()));
}

function quickConnectSection() {
  const status = el("div", { class: "status-line" });
  const actions = el("div", { class: "actions" });
  const review = el("div", { class: "review", hidden: true });
  const message = el("div", { class: "message", hidden: true });
  let pending: { claude: HooksPreview; codex: HooksPreview } | null = null;

  const say = (text: string, kind: "ok" | "error" | "info") => {
    message.hidden = false;
    message.className = `message ${kind}`;
    message.textContent = text;
  };

  async function refresh() {
    try {
      const [claude, codex] = await Promise.all([
        invoke<HooksStatus>("hooks_status"),
        invoke<HooksStatus>("codex_hooks_status"),
      ]);
      const label = (name: string, value: HooksStatus) => `${name}: ${value.state === "installed" ? "connected" : value.state === "outdated" ? "needs update" : value.state === "error" ? "check settings" : "not connected"}`;
      status.replaceChildren(el("span", { class: "muted", text: `${label("Claude Code", claude)}  ·  ${label("Codex", codex)}` }));
    } catch (error) {
      status.replaceChildren(el("span", { class: "muted", text: `Couldn't check connections: ${String(error)}` }));
    }
  }

  async function connect() {
    message.hidden = true;
    review.hidden = true;
    try {
      const [claude, codex] = await Promise.all([
        invoke<HooksPreview>("hooks_preview", { install: true }),
        invoke<HooksPreview>("codex_hooks_preview", { install: true }),
      ]);
      pending = { claude, codex };
      const content: Node[] = [
        el("h3", { text: "Review connections for Claude Code and Codex" }),
        el("p", { class: "hint", text: "Check both changes below. Glowby backs up each settings file before applying it. For both, the activity hooks run in the background; only the permission hook waits for your answer on Glowby (or auto-allow), and if Glowby is closed the normal prompt appears. For Claude, Glowby also becomes the status line (unless you have your own) so it can show your usage limits." }),
      ];
      for (const [name, preview] of [["Claude Code", claude], ["Codex", codex]] as const) {
        content.push(el("h3", { text: name }));
        content.push(el("p", { class: "hint", text: preview.settingsPath }));
        if (!preview.changed) {
          content.push(el("p", { class: "hint", text: "Already connected; no changes needed." }));
          continue;
        }
        if (preview.reformatted) content.push(el("p", { class: "hint", text: "This file's spacing will be normalised; its settings stay the same." }));
        const lines = el("pre", { class: "diff" });
        for (const line of preview.diff) {
          const cls = line.tag === "+" ? "add" : line.tag === "-" ? "del" : line.tag === "gap" ? "gap" : "ctx";
          lines.append(el("div", { class: cls, text: line.tag === "gap" ? "…" : `${line.tag} ${line.text}` }));
        }
        content.push(lines);
      }
      if (!claude.changed && !codex.changed) {
        pending = null;
        say("Claude Code and Codex are already connected.", "info");
        return;
      }
      content.push(el("div", { class: "actions" },
        button("Apply both connections", "primary", () => void apply()),
        button("Cancel", "", () => { review.hidden = true; pending = null; }),
      ));
      review.replaceChildren(...content);
      review.hidden = false;
    } catch (error) {
      say(String(error), "error");
    }
  }

  async function apply() {
    if (!pending) return;
    const { claude, codex } = pending;
    try {
      let claudeBackup = "";
      let codexBackup = "";
      if (claude.changed) claudeBackup = await invoke<string>("hooks_apply", { install: true, token: claude.token });
      if (codex.changed) codexBackup = await invoke<string>("codex_hooks_apply", { install: true, token: codex.token });
      const backups = [claudeBackup, codexBackup].filter(Boolean);
      review.hidden = true;
      pending = null;
      say(`Connected. ${backups.length ? `Backups saved: ${backups.join(" and ")}. ` : ""}Restart your Claude Code and Codex sessions. In Codex, use /hooks to review and trust Glowby's hooks.`, "ok");
      await refreshConnections();
    } catch (error) {
      say(`Connection setup stopped: ${String(error)}. Any change already applied has its own backup. Review the connection status, then try again.`, "error");
      await refreshConnections();
    }
  }

  actions.append(button("Connect Claude Code + Codex…", "primary", () => void connect()));
  connectionRefreshers.push(refresh);
  void refresh();
  return section("Connect Claude Code + Codex", "One setup for both tools. Review the exact changes before Glowby updates either settings file.", status, actions, review, message);
}

function row(label: string, hint: string | null, control: Node) {
  return el("div", { class: "row" }, el("div", { class: "label" }, el("div", { text: label }), hint ? el("div", { class: "hint", text: hint }) : null), control);
}

function toggle(label: string, hint: string | null, get: () => boolean, set: (v: boolean) => void) {
  const input = el("input", { type: "checkbox", role: "switch" });
  input.checked = get();
  input.addEventListener("change", () => {
    set(input.checked);
    save();
  });
  return row(label, hint, el("label", { class: "switch" }, input, el("span", { class: "slider" })));
}

function select<T extends string>(label: string, hint: string | null, options: [T, string][], get: () => T, set: (v: T) => void) {
  const s = el("select");
  for (const [value, text] of options) s.append(el("option", { value, text }));
  s.value = get();
  s.addEventListener("change", () => {
    set(s.value as T);
    save();
  });
  return row(label, hint, s);
}

function numberInput(label: string, hint: string, min: number, max: number, get: () => number, set: (v: number) => void, unit: string) {
  const input = el("input", { type: "number", min, max, step: 1, class: "num" });
  input.value = String(get());
  input.addEventListener("change", () => {
    const v = Math.min(max, Math.max(min, Math.round(Number(input.value) || min)));
    input.value = String(v);
    set(v);
    save();
  });
  return row(label, hint, el("span", { class: "inline" }, input, el("span", { class: "muted", text: unit })));
}

// ---------- sections ----------

function hooksSection(
  provider: string,
  description: string,
  commands: { status: string; preview: string; apply: string },
  restartNote: string,
) {
  const status = el("div", { class: "status-line" });
  const actions = el("div", { class: "actions" });
  const review = el("div", { class: "review", hidden: true });
  const message = el("div", { class: "message", hidden: true });

  const say = (text: string, kind: "ok" | "error" | "info", extra?: Node) => {
    message.hidden = false;
    message.className = `message ${kind}`;
    message.replaceChildren(text, extra ?? "");
  };

  async function refresh() {
    const st = await invoke<HooksStatus>(commands.status);
    const labels = {
      installed: ["Connected", "ok"],
      outdated: ["Needs update", "warn"],
      notInstalled: ["Not connected", "off"],
      error: ["Problem", "error"],
    } as const;
    const [text, tone] = labels[st.state];
    status.replaceChildren(
      el("span", { class: `pill ${tone}`, text }),
      el("span", { class: "muted", text: st.settingsPath }),
    );
    if (st.detail) status.append(el("div", { class: "hint", text: st.detail }));
    actions.replaceChildren(...compact(
      button(st.state === "installed" ? "Reinstall hooks…" : st.state === "outdated" ? "Update hooks…" : "Install hooks…", "primary", () => void preview(true)),
      st.state === "notInstalled" ? null : button("Uninstall hooks…", "", () => void preview(false)),
      button("Open backups folder", "ghost", () => void invoke("open_folder", { which: "backups" })),
    ));
  }

  async function preview(install: boolean) {
    message.hidden = true;
    let p: HooksPreview;
    try {
      p = await invoke<HooksPreview>(commands.preview, { install });
    } catch (e) {
      say(String(e), "error");
      return;
    }
    if (!p.changed) {
      review.hidden = true;
      say(install ? "Already up to date. Nothing to change." : "No Glowby hooks found. Nothing to remove.", "info");
      void refreshConnections(); // the status shown above may be older than the file
      return;
    }
    const lines = el("pre", { class: "diff" });
    for (const line of p.diff) {
      const cls = line.tag === "+" ? "add" : line.tag === "-" ? "del" : line.tag === "gap" ? "gap" : "ctx";
      lines.append(el("div", { class: cls, text: line.tag === "gap" ? "…" : `${line.tag} ${line.text}` }));
    }
    review.hidden = false;
    review.replaceChildren(...compact(
      el("h3", { text: install ? "Review: add Glowby's hooks" : "Review: remove Glowby's hooks" }),
      el("p", { class: "hint", text: `File: ${p.settingsPath}. Green lines are added, red lines removed. A backup is made before anything changes.` }),
      p.reformatted ? el("p", { class: "hint", text: "Note: the file's spacing will be normalised (2-space JSON). Your settings stay the same." }) : null,
      lines,
      el(
        "div",
        { class: "actions" },
        button("Apply", "primary", () => void apply(p)),
        button("Cancel", "", () => {
          review.hidden = true;
        }),
      ),
    ));
  }

  async function apply(p: HooksPreview) {
    try {
      const backup = await invoke<string>(commands.apply, { install: p.install, token: p.token });
      review.hidden = true;
      say(
        `${p.install ? "Hooks installed." : "Hooks removed."} ${backup ? `Backup saved: ${backup}.` : ""} ${restartNote}`,
        "ok",
      );
    } catch (e) {
      say(String(e), "error");
    }
    await refreshConnections();
  }

  connectionRefreshers.push(refresh);
  void refresh();
  return section(
    provider,
    description,
    status,
    actions,
    review,
    message,
  );
}

function petSection(monitors: MonitorInfo[]) {
  const monitorOptions: [string, string][] = [["", "Primary monitor"], ...monitors.map((m) => [m.name, m.label] as [string, string])];
  const slider = el("input", { type: "range", min: 0, max: 100, step: 1, class: "range" });
  slider.value = String(Math.round(settings.pet.position * 100));
  slider.addEventListener("input", () => {
    settings.pet.position = Number(slider.value) / 100;
    save();
  });
  return section(
    "Glowby",
    "Glowby hides at the top edge of your screen. Hover the top edge to call it out.",
    select("Monitor", null, monitorOptions, () => settings.pet.monitor, (v) => (settings.pet.monitor = v)),
    row("Position on the top edge", "You can also drag Glowby sideways.", el("span", { class: "inline" }, slider, button("Show me", "ghost", () => void invoke("show_pet")))),
    toggle("Eyes follow the mouse", null, () => settings.pet.followMouse, (v) => (settings.pet.followMouse = v)),
    toggle("Status line while hidden", "A thin line at the top edge: blue = working, amber = needs you.", () => settings.pet.statusLine, (v) => (settings.pet.statusLine = v)),
    toggle("Pop out for permission requests", null, () => settings.pet.showOnPermission, (v) => (settings.pet.showOnPermission = v)),
    toggle("Pop out when a task is done", null, () => settings.pet.showOnDone, (v) => (settings.pet.showOnDone = v)),
    toggle("Pop out when Claude needs you", "Questions in the terminal, errors.", () => settings.pet.showOnAttention, (v) => (settings.pet.showOnAttention = v)),
  );
}

function permissionsSection() {
  return section(
    "Permission requests",
    "When Claude Code asks for permission, Glowby shows Allow / Deny. If you don't answer in time, the question goes back to the terminal.",
    toggle("Answer permission requests on Glowby", null, () => settings.permissions.enabled, (v) => (settings.permissions.enabled = v)),
    numberInput("Hand back to the terminal after", "5 to 540 seconds.", 5, 540, () => settings.permissions.timeoutSecs, (v) => (settings.permissions.timeoutSecs = v), "seconds"),
  );
}

function usageGuardSection() {
  const d = settings.detective;
  return section(
    "Saves your usage automatically",
    "Glowby steps in before usage is wasted, instead of only telling you afterwards. Big chats are the expensive ones: Claude keeps a chat in a cache for a while (5 minutes or 1 hour), and after that your next message re-sends the whole chat at full price. Everything is checked on this PC from Claude Code's own logs.",
    toggle(
      "Stop expensive messages to cold chats",
      "When a big chat's cache has gone cold, Glowby stops your message once and puts a fresh-start note with it on your clipboard. Paste it into a new chat, or send the message again within 10 minutes to go ahead anyway. Needs the updated connection (Connect Claude Code above).",
      () => d.guard,
      (v) => (d.guard = v),
    ),
    toggle(
      "Remind me before a big chat goes cold",
      "Two minutes before, if you haven't replied, so you can answer while it's still cheap.",
      () => d.cacheReminder,
      (v) => (d.cacheReminder = v),
    ),
    numberInput("A big chat has at least", "50 to 2000 thousand tokens.", 50, 2000, () => Math.round(d.guardMinTokens / 1000), (v) => (d.guardMinTokens = v * 1000), "k tokens"),
    toggle(
      "Suggest a fresh chat when one gets very long",
      "Glowby writes the fresh-start note for you (your recent requests, the files that changed, how the last answer ended). Built on your PC, no AI, no tokens.",
      () => d.bigChat,
      (v) => (d.bigChat = v),
    ),
    numberInput("Very long means", "100 to 2000 thousand tokens.", 100, 2000, () => Math.round(d.bigChatTokens / 1000), (v) => (d.bigChatTokens = v * 1000), "k tokens"),
    toggle(
      "Windows notifications for important alerts",
      "A usage limit hit, Claude being back, a stopped message: also as a Windows notification, so you see it on any screen. Glowby jumps out with a sound either way (never during a fullscreen game).",
      () => settings.alerts.windowsNotifications,
      (v) => (settings.alerts.windowsNotifications = v),
    ),
  );
}

async function detectiveSection() {
  const report = el("pre", { class: "diff report" });
  async function load() {
    const text = await invoke<string>("detective_last");
    report.textContent = text || "No case report yet. Make one below.";
  }
  await load();
  return section(
    "Token detective",
    "Finds what eats your Claude usage limits, from your local Claude Code logs: cache rebuilds after breaks, files Claude re-reads without changes, and long sessions that get expensive. It reads numbers, times, tool names and file paths, never your messages, and nothing leaves this PC.",
    toggle("Token detective", null, () => settings.detective.enabled, (v) => (settings.detective.enabled = v)),
    toggle("Weekly case report", "On your first coding activity each week, Glowby puts on his detective hat and shows what he found.", () => settings.detective.weekly, (v) => (settings.detective.weekly = v)),
    el(
      "div",
      { class: "actions" },
      button("Make a case report now", "primary", async () => {
        await invoke("detective_run");
        report.textContent = "Investigating…";
        window.setTimeout(() => void load(), 4000);
      }),
    ),
    report,
  );
}

async function limitsSection() {
  const box = el("div");
  function render(view: LimitsView | null) {
    if (!view) {
      box.replaceChildren(el("p", { class: "hint", text: "Turned off." }));
      return;
    }
    box.replaceChildren(
      ...view.agents.map((a) =>
        el(
          "div",
          { class: "limits-agent" },
          el("h3", { text: a.plan ? `${a.agent} (${a.plan} plan)` : a.agent }),
          ...(a.windows.length
            ? a.windows.map((w) =>
                el(
                  "p",
                  { class: "hint" },
                  el("strong", { text: `${w.label}: ${w.usedText}` }),
                  ` · ${w.resetsText} · ${w.forecast} · ${w.exact ? "" : "estimate, "}${w.asOf}${w.stale ? " (may be out of date)" : ""}`,
                ),
              )
            : [el("p", { class: "hint", text: a.emptyHint })]),
        ),
      ),
    );
  }
  render(await invoke<LimitsView | null>("limits_refresh"));
  return section(
    "AI limits (Claude & Codex)",
    "How much of your Claude and Codex usage limits you've used, when they reset, and a rough guess of when you may run low. Codex's numbers are exact (from its own logs). Claude shares exact percentages only with terminal sessions (Glowby's status line); the Claude app doesn't, and claude.ai chats use the same limit. So for Claude, Glowby reliably tells you the moment you hit a limit and when it's back (Claude's own reset time), plus a token count. The \"runs low in\" time assumes you keep your recent pace, so treat it as a hint. Everything is read on this PC; nothing is sent anywhere.",
    toggle("Show AI limits", null, () => settings.limits.enabled, (v) => (settings.limits.enabled = v)),
    toggle(
      "Warn me when a limit gets tight",
      "Glowby pops out once per window and suggests saving a handoff note, or switching to the other agent if it has more left.",
      () => settings.limits.warn,
      (v) => (settings.limits.warn = v),
    ),
    numberInput("Warn at", "50 to 98 percent.", 50, 98, () => settings.limits.warnPercent, (v) => (settings.limits.warnPercent = v), "%"),
    box,
    el(
      "div",
      { class: "actions" },
      button("Refresh", "ghost", async () => {
        await invoke("limits_refresh");
        window.setTimeout(async () => render(await invoke<LimitsView | null>("limits_refresh")), 1500);
      }),
    ),
  );
}

function autoAllowSection() {
  const a = settings.autoAllow;
  const timed = el("div", { class: "actions" });
  const logBox = el("div", { class: "auto-log" });

  async function renderLog() {
    const entries = await invoke<AutoAllowEntry[]>("auto_allow_log");
    logBox.replaceChildren(
      ...compact(
      el("h3", { text: `Auto-allowed for you (${entries.length})` }),
      entries.length
        ? el(
            "ul",
            { class: "quest-list" },
            ...entries.slice(0, 40).map((e) =>
              el("li", { class: "hint", text: `${new Date(e.at).toLocaleString()} · ${e.project || "?"} · ${e.what}${e.mode === "full" ? " (full auto)" : ""}` }),
            ),
          )
        : el("p", { class: "hint", text: "Nothing yet." }),
      entries.length ? el("div", { class: "actions" }, button("Clear the log", "ghost", async () => { await invoke("auto_allow_clear_log"); void renderLog(); })) : null,
      ),
    );
  }

  timed.append(
    ...[15, 30, 60].map((m) => button(m === 60 ? "Auto-allow for 1 hour" : `Auto-allow for ${m} min`, "", () => void invoke("auto_allow_start", { minutes: m }))),
    button("Stop", "ghost", () => void invoke("auto_allow_stop")),
  );

  const never = el("textarea", { class: "text prompt", rows: 8, spellcheck: "false" });
  never.value = a.never.join("\n");
  never.addEventListener("input", () => {
    a.never = never.value.split("\n").map((l) => l.trim()).filter(Boolean);
    save();
  });
  const reset = button("Reset to the safe defaults", "ghost", async () => {
    a.never = await invoke<string[]>("auto_allow_defaults");
    never.value = a.never.join("\n");
    save();
  });

  void renderLog();
  return section(
    "Auto-allow (Claude Code & Codex)",
    "Let Glowby answer Claude Code's and Codex's permission questions with Allow for you: for a while (also from the right-click menu), or always with full auto. Anything on the never-auto list still asks you. Codex needs its connection updated once (Connect to Codex → Update) and the hooks trusted with /hooks.",
    row("Timed auto-allow", "Turns itself off when the time is up, and when Glowby restarts.", timed),
    toggle(
      "Full auto",
      "Off by default. Allows every question (except the never-auto list) until you turn this off. Only use it for projects you trust.",
      () => a.full,
      (v) => (a.full = v),
    ),
    toggle("Always ask for file changes outside the project folder", null, () => a.outsideProject, (v) => (a.outsideProject = v)),
    el("h3", { text: "Never auto-allow (always ask)" }),
    el("p", { class: "hint", text: "One rule per line, matched as whole words in the command or tool, e.g. \"git push\" or \"rm\"." }),
    never,
    el("div", { class: "actions" }, reset),
    logBox,
  );
}

function chatSection(info: AppInfo) {
  const folder = el("input", { type: "text", class: "text", placeholder: "C:\\path\\to\\your\\project", spellcheck: "false" });
  folder.value = settings.chat.projectDir;
  folder.addEventListener("change", () => {
    settings.chat.projectDir = folder.value.trim();
    save();
  });
  const browse = button("Browse…", "", async () => {
    const picked = await open({ directory: true, multiple: false, title: "Choose your project folder" });
    if (typeof picked === "string") {
      folder.value = picked;
      settings.chat.projectDir = picked;
      save();
    }
  });
  const claudePath = el("input", { type: "text", class: "text", placeholder: info.claudePath ?? "claude.exe not found", spellcheck: "false" });
  claudePath.value = settings.chat.claudePath;
  claudePath.addEventListener("change", () => {
    settings.chat.claudePath = claudePath.value.trim();
    save();
  });
  return section(
    "Chat",
    "Click Glowby to chat with the agent you are using. Auto follows recent Claude Code or Codex activity and uses your existing login. Conversations stay separate for each agent and project. Codex Ask mode is read-only; choose Accept edits to grant project write access.",
    toggle("Chat", null, () => settings.chat.enabled, (v) => (settings.chat.enabled = v)),
    select<"auto" | "claude" | "codex">("Chat agent", "Auto switches with your latest active session.", [["auto", "Auto · follow my active agent"], ["claude", "Claude Code"], ["codex", "Codex"]], () => settings.chat.agent, v => settings.chat.agent = v),
    row("Project folder", "Only use folders you trust.", el("span", { class: "inline grow" }, folder, browse)),
    select<ChatMode>(
      "What chat may do",
      null,
      [
        ["ask", "Claude asks on Glowby · Codex reads only"],
        ["acceptEdits", "Allow project edits · commands follow agent permissions"],
        ["readOnly", "Read only (plan mode)"],
      ],
      () => settings.chat.mode,
      (v) => (settings.chat.mode = v),
    ),
    toggle("Keep one conversation per project", "Use \"New chat\" in the bubble to start over.", () => settings.chat.keepConversation, (v) => (settings.chat.keepConversation = v)),
    row("claude.exe location", "Leave empty to find it automatically.", el("span", { class: "inline grow" }, claudePath)),
  );
}

function quickActionsSection() {
  const list = el("div", { class: "qa-list" });

  function renderList() {
    list.replaceChildren(
      ...settings.quickActions.actions.map((action, index) => {
        const label = el("input", { type: "text", class: "text", value: action.label, maxlength: 60, placeholder: "Menu label" });
        label.addEventListener("input", () => {
          action.label = label.value;
          save();
        });
        const prompt = el("textarea", { class: "text prompt", rows: 3, maxlength: 4000, placeholder: "What should Claude do?" });
        prompt.value = action.prompt;
        prompt.addEventListener("input", () => {
          action.prompt = prompt.value;
          save();
        });
        const readOnly = el("input", { type: "checkbox" });
        readOnly.checked = action.readOnly;
        readOnly.addEventListener("change", () => {
          action.readOnly = readOnly.checked;
          save();
        });
        const move = (delta: number) => () => {
          const list = settings.quickActions.actions;
          const target = index + delta;
          if (target < 0 || target >= list.length) return;
          [list[index], list[target]] = [list[target], list[index]];
          save();
          renderList();
        };
        return el(
          "div",
          { class: "qa-item" },
          el(
            "div",
            { class: "inline" },
            label,
            el("label", { class: "check", title: "Glowby blocks file edits for this action" }, readOnly, "Read only"),
            button("↑", "ghost", move(-1), "Move up"),
            button("↓", "ghost", move(1), "Move down"),
            button("Delete", "ghost danger", () => {
              settings.quickActions.actions.splice(index, 1);
              save();
              renderList();
            }),
          ),
          prompt,
        );
      }),
    );
  }

  const add = button("Add action", "", () => {
    settings.quickActions.actions.push({ id: crypto.randomUUID(), label: "New action", prompt: "", readOnly: false });
    save();
    renderList();
  });
  const reset = button("Reset to defaults", "ghost", async () => {
    settings.quickActions.actions = await invoke("default_quick_actions");
    save();
    renderList();
  });
  renderList();
  return section(
    "Quick actions",
    "Right-click Glowby to run these in your project. In a prompt, {last_error} becomes the last error Glowby saw, {project} the project name and {today} today's date.",
    toggle("Quick actions menu", null, () => settings.quickActions.enabled, (v) => (settings.quickActions.enabled = v)),
    list,
    el("div", { class: "actions" }, add, reset),
  );
}

function helpersSection() {
  return section(
    "Helpers",
    null,
    toggle(
      "Drop files on Glowby",
      "Drag a file to the top edge where Glowby lives, drop it on him, then say what to do with it.",
      () => settings.dropFiles,
      (v) => (settings.dropFiles = v),
    ),
    toggle(
      "Error watcher",
      "Off by default. When on, Glowby checks text you copy and offers help if it looks like an error. Checked on this PC only; anything that isn't an error is ignored and forgotten right away, and nothing is logged. Content password managers mark as private is skipped.",
      () => settings.errorWatcher,
      (v) => (settings.errorWatcher = v),
    ),
    toggle(
      "Look sick while tests or builds fail",
      "Glowby notices when Claude runs your tests or build. It stays sick until they pass again.",
      () => settings.health,
      (v) => (settings.health = v),
    ),
    el("div", { class: "actions" }, button("I'm fine now (clear sick state)", "ghost", () => void invoke("health_clear"))),
  );
}

function canvasPreview(appearance: Look, mood: "happy" | "idle", faded = false) {
  const c = el("canvas", { class: faded ? "preview faded" : "preview" });
  new PetRenderer(c).drawStill(mood, { ...DEFAULT_APPEARANCE, ...appearance, weak: false });
  return c;
}

/** The look you picked, limited to what's unlocked and to characters that still exist. */
function chosenLook(info: ProgressInfo): Look {
  const owned = (id: string) => info.cosmetics.some((c) => c.id === id && c.unlocked);
  return {
    ...info.look,
    hat: owned(settings.progression.hat) ? settings.progression.hat : "",
    color: owned(settings.progression.color) ? settings.progression.color : "periwinkle",
    aura: owned(settings.progression.aura) ? settings.progression.aura : "",
    character: characters.some((c) => c.id === settings.pet.character) ? settings.pet.character : "",
    species: owned(settings.progression.pet) ? settings.progression.pet : "",
  };
}

async function progressSection() {
  const info = await invoke<ProgressInfo>("get_progress");
  const card = section("Glowby's progress", "Glowby earns XP when Claude finishes tasks, when failing tests or builds pass again, for passing tests and commits, and for keeping a daily streak.");
  const body = el("div");
  card.append(body);

  function render(info: ProgressInfo) {
    const v = info.view;
    const look = chosenLook(info);
    const pct = Math.round((v.xpIntoLevel / Math.max(1, v.xpForLevel)) * 100);

    const stages = info.stages.map(([level, name], i) =>
      el(
        "div",
        { class: "stage" + (i === v.stage ? " current" : "") },
        canvasPreview({ ...look, stage: i }, "idle", i > v.stage),
        el("div", { class: "hint", text: i > v.stage ? `${name} · Lv ${level}` : name }),
      ),
    );

    const picker = (kind: "hat" | "color" | "aura") => {
      const items = info.cosmetics.filter((c) => c.kind === kind);
      const p = settings.progression;
      const current = kind === "hat" ? p.hat : kind === "color" ? p.color : p.aura;
      const choose = (id: string) => () => {
        if (kind === "hat") p.hat = id;
        else if (kind === "color") p.color = id;
        else p.aura = id;
        save();
        render(info);
      };
      const none = kind === "hat" ? "No hat" : kind === "aura" ? "No aura" : null;
      return el(
        "div",
        { class: "picker" },
        ...(none ? [button(none, current === "" ? "primary" : "", choose(""))] : []),
        ...items.map((c) => {
          const b = button(c.unlocked ? c.name : `${c.name} · ${c.requirement}`, c.id === current ? "primary" : "", choose(c.id));
          b.disabled = !c.unlocked;
          return b;
        }),
      );
    };
    const emotes = el(
      "div",
      { class: "picker" },
      ...info.cosmetics
        .filter((c) => c.kind === "emote")
        .map((c) => {
          const b = button(c.unlocked ? `Play ${c.name}` : `${c.name} · ${c.requirement}`, "", () => void invoke("play_emote", { id: c.id }));
          b.disabled = !c.unlocked;
          return b;
        }),
    );

    body.replaceChildren(
      el(
        "div",
        { class: "progress-head" },
        canvasPreview(look, "happy"),
        el(
          "div",
          { class: "progress-text" },
          el("div", { class: "big", text: `Level ${v.level} · ${v.stageName}` }),
          el("div", { class: "xpbar" }, el("span", { style: `width:${pct}%` })),
          el("div", { class: "hint", text: `${v.xpIntoLevel} / ${v.xpForLevel} XP to level ${v.level + 1} · ${v.xp} XP total` }),
          el("div", { class: "hint", text: `Streak: ${v.streak} day${v.streak === 1 ? "" : "s"} (best ${info.bestStreak}) · Energy ${v.energy}%` }),
          el(
            "div",
            {
              class: "hint",
              text: `Tasks ${info.stats.tasks} · Fixes ${info.stats.fixes} · Test runs passed ${info.stats.testsPassed} · Commits ${info.stats.commits} · Breaks ${info.stats.breaks} · Petted ${info.stats.pets ?? 0} times`,
            },
          ),
        ),
      ),
      el("h3", { text: "Evolution" }),
      el("div", { class: "stages" }, ...stages),
      el("h3", { text: "Hat" }),
      picker("hat"),
      el("h3", { text: "Colour" }),
      picker("color"),
      el("h3", { text: "Aura" }),
      el("p", { class: "hint", text: "Power-up effects around Glowby or your character. New ones unlock as you level up." }),
      picker("aura"),
      el("h3", { text: "Emotes" }),
      emotes,
      toggle("Earn XP and level up", null, () => settings.progression.enabled, (v) => (settings.progression.enabled = v)),
      toggle(
        "Get sleepy when ignored",
        "If you don't code or visit Glowby for a few days he gets tired and pale. He never dies, and perks up when you're back.",
        () => settings.progression.neglect,
        (v) => (settings.progression.neglect = v),
      ),
    );
  }
  render(info);
  refreshProgress = () => render(info);
  return card;
}

function charactersSection() {
  const grid = el("div", { class: "char-grid" });
  const cropHost = el("div", { class: "crop-host", hidden: true });
  const message = el("div", { class: "hint" });
  let current: Look = { ...DEFAULT_APPEARANCE };
  /** The anime pets (unlocked or not) from Glowby's progress. */
  let pets: CosmeticView[] = [];

  /** What Glowby wears now: "" (jellyfish), "p:<pet>" or "c:<character>". */
  function currentKey() {
    if (characters.some((c) => c.id === settings.pet.character)) return `c:${settings.pet.character}`;
    if (pets.some((p) => p.id === settings.progression.pet && p.unlocked)) return `p:${settings.progression.pet}`;
    return "";
  }

  function use(key: string) {
    if (key.startsWith("c:")) settings.pet.character = key.slice(2);
    else {
      settings.pet.character = "";
      settings.progression.pet = key.startsWith("p:") ? key.slice(2) : "";
    }
    save();
    renderGrid();
    refreshProgress();
  }

  async function remove(c: CharacterInfo) {
    try {
      await invoke("character_delete", { id: c.id });
      characters = characters.filter((x) => x.id !== c.id);
      if (settings.pet.character === c.id) settings.pet.character = "";
      renderGrid();
      refreshProgress();
    } catch (e) {
      message.textContent = String(e);
    }
  }

  function renderGrid() {
    const now = currentKey();
    const tile = (key: string, look: Partial<Look>, label: Node, faded: boolean, ...extra: Node[]) =>
      el(
        "div",
        { class: "char" + (now === key ? " current" : "") },
        canvasPreview({ ...current, character: "", species: "", ...look }, "happy", faded),
        label,
        el("div", { class: "actions" }, ...extra),
      );
    const useButton = (key: string, label = "Use") => button(now === key ? "In use" : label, now === key ? "primary" : "", () => use(key));
    const jelly = tile("", {}, el("div", { class: "char-name", text: "Glowby (jellyfish)" }), false, useButton(""));
    const petTiles = pets.map((p) => {
      const key = `p:${p.id}`;
      const action = p.unlocked ? useButton(key) : button(`Unlocks at ${p.requirement}`, "", () => {});
      if (!p.unlocked) (action as HTMLButtonElement).disabled = true;
      return tile(key, { species: p.id }, el("div", { class: "char-name", text: p.name }), !p.unlocked, action);
    });
    const tiles = characters.map((c) => {
      const name = el("input", { type: "text", class: "text char-name", value: c.name, maxlength: 40, title: "Rename" });
      name.addEventListener("change", async () => {
        try {
          await invoke("character_rename", { id: c.id, name: name.value });
          c.name = name.value.trim() || c.name;
        } catch (e) {
          message.textContent = String(e);
        }
      });
      // Two clicks to remove, so a stray click can't lose a character.
      let armed = 0;
      const removeBtn = button("Remove", "ghost danger", () => {
        if (armed) {
          window.clearTimeout(armed);
          void remove(c);
          return;
        }
        removeBtn.textContent = "Click again to remove";
        armed = window.setTimeout(() => {
          armed = 0;
          removeBtn.textContent = "Remove";
        }, 4000);
      });
      return tile(`c:${c.id}`, { character: c.id }, name, false, useButton(`c:${c.id}`, "Use for Glowby"), removeBtn);
    });
    grid.replaceChildren(jelly, ...petTiles, ...tiles);
  }

  async function importPicture() {
    message.textContent = "";
    const path = await open({
      multiple: false,
      title: "Choose a picture of your character",
      filters: [{ name: "Pictures", extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp"] }],
    });
    if (typeof path !== "string") return;
    let bitmap: ImageBitmap;
    try {
      const bytes = await invoke<ArrayBuffer>("character_read_source", { path });
      bitmap = await createImageBitmap(new Blob([bytes]));
    } catch (e) {
      message.textContent = typeof e === "string" ? e : "Couldn't read that picture. Try a PNG or JPG.";
      return;
    }
    const result = await crop(cropHost, bitmap, nameFromFile(path));
    bitmap.close();
    if (!result) return;
    try {
      const bytes = new Uint8Array(await result.png.arrayBuffer());
      // The PNG goes as the raw request body; the name rides in a header (ASCII-encoded).
      const info = await invoke<CharacterInfo>("character_add", bytes, { headers: { "x-name": encodeURIComponent(result.name) } });
      setCharacterImage(info.id, await createImageBitmap(result.png));
      characters = [...characters, info];
      message.textContent = `${info.name} is ready and now in use. Pick another look anytime.`;
      use(`c:${info.id}`);
    } catch (e) {
      message.textContent = String(e);
    }
  }

  void invoke<ProgressInfo>("get_progress").then((info) => {
    current = { ...chosenLook(info), character: "", species: "" };
    pets = info.cosmetics.filter((c) => c.kind === "pet");
    renderGrid();
  });
  renderGrid();
  return section(
    "Pets and characters",
    "Pick who lives at the top of your screen: Glowby the jellyfish, one of his anime-style friends (new ones unlock as you level up), or a picture of any character you like (an anime hero, a game character, your cat) shown as a round icon. They all have Glowby's moods, hats, emotes and auras. Stroke them with the mouse to pet them! Pictures stay on this PC; squad pets can wear all of these too.",
    el("div", { class: "actions" }, button("Import a picture…", "primary", () => void importPicture())),
    cropHost,
    message,
    grid,
  );
}

function squadSection() {
  return section(
    "Squad mode",
    "One small pet for each running Claude Code session, next to Glowby. Each pet levels up on its own from what its session does (tool uses, finished tasks, commits, fixes). Click a pet to see what it's doing, change its look, or chat with it. Chat talks to a copy of the session, so the one in your terminal is never disturbed.",
    toggle("Squad mode", "Off by default.", () => settings.squad.enabled, (v) => (settings.squad.enabled = v)),
    numberInput("Show at most", "1 to 6 pets.", 1, 6, () => settings.squad.maxShown, (v) => (settings.squad.maxShown = v), "pets"),
  );
}

function breaksSection() {
  return section(
    "Break reminder",
    "After a long stretch of coding, Glowby gets sleepy and suggests a short rest. A pause of 15 minutes counts as a break.",
    toggle("Remind me to take breaks", null, () => settings.breaks.enabled, (v) => (settings.breaks.enabled = v)),
    numberInput("Remind me after", "15 to 240 minutes of continuous coding.", 15, 240, () => settings.breaks.intervalMins, (v) => (settings.breaks.intervalMins = v), "minutes"),
  );
}

function soundsSection() {
  const slider = el("input", { type: "range", min: 0, max: 100, step: 5, class: "range" });
  slider.value = String(settings.sounds.volume);
  slider.addEventListener("input", () => {
    settings.sounds.volume = Number(slider.value);
    save();
  });
  slider.addEventListener("change", () => playSound("done", settings.sounds.volume / 100));
  const s = settings.sounds;
  return section(
    "Sounds",
    "Little chimes made in code (no sound files). Always muted while a fullscreen game runs.",
    toggle("Sounds", null, () => s.enabled, (v) => (s.enabled = v)),
    row("Volume", null, el("span", { class: "inline" }, slider, button("Test", "ghost", () => playSound("levelup", s.volume / 100)))),
    toggle("Task finished", null, () => s.taskDone, (v) => (s.taskDone = v)),
    toggle("Claude needs you", "Permission questions and notifications.", () => s.needsYou, (v) => (s.needsYou = v)),
    toggle("Problems", "Failing tests or builds, errors.", () => s.problems, (v) => (s.problems = v)),
    toggle("Level up", null, () => s.levelUp, (v) => (s.levelUp = v)),
    toggle("Break reminder", null, () => s.breaks, (v) => (s.breaks = v)),
  );
}

function briefingSection() {
  return section(
    "Daily briefing",
    "On your first activity of the day: what you committed yesterday, what's unfinished (uncommitted files, unpushed commits, failing tests) and one small next step. Worked out locally from git.",
    toggle("Daily briefing", null, () => settings.briefing.enabled, (v) => (settings.briefing.enabled = v)),
    el("div", { class: "actions" }, button("Show today's briefing now", "ghost", () => void invoke("briefing_show"))),
  );
}

function learnSection() {
  return section(
    "Learn mode",
    "After Claude changes your code, Glowby asks one short multiple-choice question about what changed and why. Right answers give +15 XP. The question is written by Claude's small model (Haiku) through your Claude login, so it uses a little of your Claude usage. Tools and hooks are switched off for that request.",
    toggle("Learn mode", null, () => settings.learn.enabled, (v) => (settings.learn.enabled = v)),
    numberInput("At most one question every", "5 to 240 minutes.", 5, 240, () => settings.learn.everyMins, (v) => (settings.learn.everyMins = v), "minutes"),
  );
}

const QUEST_KINDS: [string, string][] = [
  ["fix", "Fix bugs (failing tests or builds that pass again)"],
  ["test", "Write tests"],
  ["minutes", "Code for a while"],
  ["tasks", "Finish tasks with Claude"],
  ["commit", "Make commits"],
  ["learn", "Answer learn-mode questions (needs learn mode)"],
  ["break", "Take breaks (needs break reminders)"],
  ["pet", "Pet Glowby (stroke him with the mouse)"],
];

async function questsSection() {
  const today = el("div");
  async function refreshToday() {
    const quests = await invoke<{ label: string; progress: number; target: number; done: boolean; xp: number }[]>("quests_today");
    today.replaceChildren(
      el("h3", { text: "Today" }),
      quests.length
        ? el("ul", { class: "quest-list" }, ...quests.map((q) => el("li", { text: `${q.done ? "✓" : "○"} ${q.label} · ${q.progress}/${q.target} · ${q.xp} XP` })))
        : el("p", { class: "hint", text: "No quests today (turned off, or no quest types selected)." }),
      el("p", { class: "hint", text: "Changes to difficulty and types apply from tomorrow's quests." }),
    );
  }
  await refreshToday();
  const kinds = el(
    "div",
    { class: "picker" },
    ...QUEST_KINDS.map(([kind, label]) => {
      const box = el("input", { type: "checkbox" });
      box.checked = settings.quests.kinds.includes(kind);
      box.addEventListener("change", () => {
        settings.quests.kinds = box.checked ? [...settings.quests.kinds, kind] : settings.quests.kinds.filter((k) => k !== kind);
        save();
      });
      return el("label", { class: "check" }, box, label);
    }),
  );
  return section(
    "Daily quests",
    "Small goals for each day with XP rewards, plus a bonus when you finish them all.",
    toggle("Daily quests", null, () => settings.quests.enabled, (v) => (settings.quests.enabled = v)),
    select(
      "Difficulty",
      null,
      [
        ["easy", "Easy (15 XP each)"],
        ["normal", "Normal (25 XP each)"],
        ["hard", "Hard (40 XP each)"],
      ],
      () => settings.quests.difficulty,
      (v) => (settings.quests.difficulty = v),
    ),
    numberInput("Quests per day", "1 to 5.", 1, 5, () => settings.quests.perDay, (v) => (settings.quests.perDay = v), "quests"),
    row("Quest types", null, kinds),
    today,
  );
}

async function githubSection() {
  const status = el("div", { class: "gh-status" });
  const token = el("input", { type: "password", class: "text", placeholder: "github_pat_… (fine-grained token, Actions: read)", autocomplete: "off", spellcheck: "false" });
  const message = el("div", { class: "hint" });

  function renderStatus(s: GithubStatus) {
    status.replaceChildren(
      el("div", { class: "hint", text: s.hasToken ? "A token is stored in Windows Credential Manager." : "No token stored." }),
      ...(s.repos.length
        ? s.repos.map((r) =>
            el("div", { class: "hint", text: `${r.repo} (${r.branch}): ${r.state === "none" ? "no runs" : r.state}${r.checked ? ` · checked ${r.checked}` : ""}` }),
          )
        : [el("div", { class: "hint", text: "Repositories: found from the git remotes of your recent Claude Code projects." })]),
    );
  }
  renderStatus(await invoke<GithubStatus>("github_status"));

  const saveBtn = button("Save token", "", async () => {
    try {
      await invoke("github_save_token", { token: token.value });
      token.value = ""; // never kept in the page
      message.textContent = "Saved to Windows Credential Manager.";
      renderStatus(await invoke<GithubStatus>("github_status"));
    } catch (e) {
      message.textContent = String(e);
    }
  });
  const removeBtn = button("Remove token", "ghost", async () => {
    await invoke("github_remove_token");
    message.textContent = "Token removed.";
    renderStatus(await invoke<GithubStatus>("github_status"));
  });
  const checkBtn = button("Check now", "ghost", async () => {
    message.textContent = "Checking…";
    renderStatus(await invoke<GithubStatus>("github_check_now"));
    message.textContent = "";
  });
  return section(
    "GitHub CI check",
    "Optional. If a GitHub Actions run fails on your current branch, Glowby tells you and can ask Claude to fix it. Only while this is on, Glowby contacts api.github.com, nothing else. Your token is stored in Windows Credential Manager, never in a file.",
    toggle("Check my GitHub CI", null, () => settings.github.enabled, (v) => (settings.github.enabled = v)),
    numberInput("Check every", "5 to 180 minutes (also a few minutes after a git push).", 5, 180, () => settings.github.everyMins, (v) => (settings.github.everyMins = v), "minutes"),
    row("Token", "Create one at github.com → Settings → Developer settings → Fine-grained tokens, with read access to Actions.", el("span", { class: "inline grow" }, token, saveBtn)),
    el("div", { class: "actions" }, removeBtn, checkBtn),
    message,
    status,
  );
}

function performanceSection(info: AppInfo) {
  return section(
    "Performance",
    "Glowby only animates while you can see it and uses no timers while hidden.",
    toggle(
      "Game mode",
      `Hide Glowby while a fullscreen app or game runs. Fullscreen app right now: ${info.fullscreenNow ? "yes" : "no"}.`,
      () => settings.gameMode,
      (v) => (settings.gameMode = v),
    ),
  );
}

function moodsSection() {
  const moods = ["idle", "working", "happy", "alert", "sleepy", "sick"];
  return section(
    "Try the moods",
    "Shows Glowby in each mood for 5 seconds.",
    el("div", { class: "actions" }, ...moods.map((m) => button(m[0].toUpperCase() + m.slice(1), "", () => void invoke("preview_mood", { mood: m })))),
  );
}

async function pulseSection() {
  const loaded = await invoke<PulseView>("pulse_view");
  const prefs = loaded.preferences;
  const persist = (key: "enabled" | "background", value: boolean) => {
    prefs[key] = value;
    void invoke<PulsePreferences>("pulse_preferences", { patch: { [key]: value } }).catch(e => { status.textContent = String(e); status.className = "message error"; });
  };
  const status = el("p", { class: "message", role: "status" });
  return section("AI Pulse", "A clean workspace for AI news, sourced model comparisons and tools. Public news checks happen when you use the hub; background checks are optional and never upload your projects or searches.",
    toggle("Enable AI Pulse", null, () => prefs.enabled, v => persist("enabled", v)),
    toggle("Background news checks", "Off by default. Check public sources every few hours; manage follows and alerts inside AI Pulse.", () => prefs.background, v => persist("background", v)),
    el("div", { class: "actions" }, button("Open AI Pulse", "primary", () => void invoke("pulse_open").catch(e => { status.textContent = String(e); status.className = "message error"; }))), status
  );
}

function privacySection(info: AppInfo) {
  return section(
    "Privacy and data",
    "Settings, backups, bookmarks and session IDs stay on your PC. AI Pulse fetches public news and benchmarks when used or when you enable background checks. Chat uses your selected agent’s service; optional GitHub CI contacts GitHub. No telemetry or project uploads from AI Pulse.",
    el("div", { class: "inline" }, el("code", { text: info.dataDir }), button("Open data folder", "ghost", () => void invoke("open_folder", { which: "data" }))),
    info.pipeError ? el("p", { class: "message error", text: info.pipeError }) : null,
    el("p", { class: "hint", text: `Version ${info.version}.` }),
  );
}

async function main() {
  const logo = document.getElementById("logo") as HTMLCanvasElement;
  new PetRenderer(logo).drawStill("happy");

  const [loaded, monitors, info, chars] = await Promise.all([
    invoke<Settings>("get_settings"),
    invoke<MonitorInfo[]>("list_monitors"),
    invoke<AppInfo>("app_info"),
    invoke<CharacterInfo[]>("characters_list"),
  ]);
  settings = loaded;
  characters = chars;
  app.replaceChildren(
    quickConnectSection(),
    await pulseSection(),
    hooksSection(
      "Connect to Claude Code",
      "Glowby listens through hooks in your Claude Code settings. Hooks fail open: if Glowby is closed or crashes, Claude Code keeps working normally.",
      { status: "hooks_status", preview: "hooks_preview", apply: "hooks_apply" },
      "Restart running Claude Code sessions to pick this up.",
    ),
    hooksSection(
      "Connect to Codex",
      "Glowby follows Codex through hooks. The activity hooks run in the background and never slow Codex down. The permission hook lets you answer Codex's questions on Glowby (or auto-allow them); if Glowby is closed or doesn't answer, Codex shows its normal prompt.",
      { status: "codex_hooks_status", preview: "codex_hooks_preview", apply: "codex_hooks_apply" },
      "Restart running Codex sessions, then use /hooks to review and trust Glowby's hooks.",
    ),
    await progressSection(),
    charactersSection(),
    squadSection(),
    petSection(monitors),
    permissionsSection(),
    autoAllowSection(),
    usageGuardSection(),
    await limitsSection(),
    await detectiveSection(),
    chatSection(info),
    quickActionsSection(),
    helpersSection(),
    breaksSection(),
    soundsSection(),
    briefingSection(),
    learnSection(),
    await questsSection(),
    await githubSection(),
    performanceSection(info),
    moodsSection(),
    privacySection(info),
  );
}

void main();
