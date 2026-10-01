// Settings window. Every change saves automatically (no "Save" button).

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { PetRenderer } from "../pet/renderer";
import { button, compact, el } from "../shared/dom";
import type { AppInfo, ChatMode, HooksPreview, HooksStatus, MonitorInfo, Settings } from "../shared/types";

const app = document.getElementById("app")!;
let settings: Settings;
let saveTimer = 0;

function save() {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(async () => {
    settings = await invoke<Settings>("save_settings", { settings });
  }, 250);
}

// ---------- small building blocks ----------

function section(title: string, intro?: string, ...body: (Node | null)[]) {
  return el("section", { class: "card" }, el("h2", { text: title }), intro ? el("p", { class: "intro", text: intro }) : null, ...body);
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

function hooksSection() {
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
    const st = await invoke<HooksStatus>("hooks_status");
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
      p = await invoke<HooksPreview>("hooks_preview", { install });
    } catch (e) {
      say(String(e), "error");
      return;
    }
    if (!p.changed) {
      review.hidden = true;
      say(install ? "Already up to date. Nothing to change." : "No Glowby hooks found. Nothing to remove.", "info");
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
      const backup = await invoke<string>("hooks_apply", { install: p.install, token: p.token });
      review.hidden = true;
      say(
        `${p.install ? "Hooks installed." : "Hooks removed."} ${backup ? `Backup saved: ${backup}.` : ""} Restart running Claude Code sessions to pick this up.`,
        "ok",
      );
    } catch (e) {
      say(String(e), "error");
    }
    await refresh();
  }

  void refresh();
  return section(
    "Connect to Claude Code",
    "Glowby listens through hooks in your Claude Code settings. Hooks fail open: if Glowby is closed or crashes, Claude Code keeps working normally.",
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
    "Click Glowby to send a message to Claude Code (non-interactive mode) in your project folder, using your normal Claude login.",
    toggle("Chat", null, () => settings.chat.enabled, (v) => (settings.chat.enabled = v)),
    row("Project folder", "Only use folders you trust.", el("span", { class: "inline grow" }, folder, browse)),
    select<ChatMode>(
      "What chat may do",
      null,
      [
        ["ask", "Ask me on Glowby before edits and commands"],
        ["acceptEdits", "Edit files freely, ask before commands"],
        ["readOnly", "Read only (plan mode)"],
      ],
      () => settings.chat.mode,
      (v) => (settings.chat.mode = v),
    ),
    toggle("Keep one conversation per project", "Use \"New chat\" in the bubble to start over.", () => settings.chat.keepConversation, (v) => (settings.chat.keepConversation = v)),
    row("claude.exe location", "Leave empty to find it automatically.", el("span", { class: "inline grow" }, claudePath)),
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

function privacySection(info: AppInfo) {
  return section(
    "Privacy and data",
    "Glowby sends nothing anywhere. Your settings, backups, and chat session IDs live only in this folder. (The chat itself talks to Anthropic through Claude Code, as Claude Code always does.)",
    el("div", { class: "inline" }, el("code", { text: info.dataDir }), button("Open data folder", "ghost", () => void invoke("open_folder", { which: "data" }))),
    info.pipeError ? el("p", { class: "message error", text: info.pipeError }) : null,
    el("p", { class: "hint", text: `Version ${info.version}. Coming in later phases: quick actions, error watcher, sounds, quests, briefing, learn mode, GitHub check, squad mode.` }),
  );
}

async function main() {
  const logo = document.getElementById("logo") as HTMLCanvasElement;
  new PetRenderer(logo).drawStill("happy");

  const [loaded, monitors, info] = await Promise.all([
    invoke<Settings>("get_settings"),
    invoke<MonitorInfo[]>("list_monitors"),
    invoke<AppInfo>("app_info"),
  ]);
  settings = loaded;
  app.replaceChildren(
    hooksSection(),
    petSection(monitors),
    permissionsSection(),
    chatSection(info),
    performanceSection(info),
    moodsSection(),
    privacySection(info),
  );
}

void main();
