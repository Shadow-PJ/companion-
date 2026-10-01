// The speech bubble under Glowby. It shows one thing at a time, by priority:
//   1. a permission question (Allow / Deny)
//   2. "drop it on me" while you drag a file over Glowby
//   3. the quick-actions menu (right-click)
//   4. the chat box (when you clicked Glowby or dropped a file)
//   5. an offer ("That looks like an error. Fix it?", failing tests)
//   6. a short message (task done, needs you, …)
//   7. the live status ("my-app · Editing main.rs")

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { diag } from "../shared/diag";
import { button, compact, el } from "../shared/dom";
import type { Offer, PermView, PetView, ProgressView } from "../shared/types";

const PHASE_LABEL: Record<string, string> = {
  idle: "Idle",
  thinking: "Thinking",
  working: "Working",
  needsYou: "Needs you",
  done: "Done",
  failed: "Hit a problem",
};

/** One-click instructions for dropped files. */
const FILE_SUGGESTIONS: [string, string][] = [
  ["Explain it", "Explain what this is and how it works, in plain language."],
  ["Review it", "Review this for bugs, risks, and unclear parts. Suggest concrete improvements, but don't change anything yet."],
  ["Fix problems", "Find and fix the problems in this. Keep changes minimal and tell me what you changed."],
  ["Summarize", "Summarize this in a few short bullet points."],
];

export class Bubble {
  /** Set by main.ts: plays an emote. */
  onEmote: (id: string) => void = () => {};
  private view: PetView | null = null;
  private menuOpen = false;
  private permBox = el("div", { class: "perm" });
  private dropBox = el("div", { class: "note drop" });
  private menuBox = el("div", { class: "menu" });
  private offerBox = el("div", { class: "offer" });
  private noteBox = el("div", { class: "note" });
  private chatBox: HTMLElement;
  private permKey = "";
  private noteKey = "";
  private offerKey = "";
  private menuKey = "";
  private deadline = 0;
  private countdown: HTMLElement | null = null;
  private countdownTimer = 0;

  // chat parts (built once so typing is never interrupted by updates)
  private picking = false;
  private chatProject = button("", "chip", () => void this.pickFolder());
  private chatNewBtn = button("New chat", "ghost", () => void invoke("chat_new"), "Forget this conversation");
  private chatTitle = el("div", { class: "asked" });
  private chatFiles = el("div", { class: "files" });
  private chatSuggest = el("div", { class: "actions suggest" });
  private chatReply = el("div", { class: "reply" });
  private chatActivity = el("div", { class: "activity" });
  private chatActivityText = el("span");
  private chatError = el("div", { class: "error" });
  private chatNotice = el("div", { class: "notice" });
  private chatInput = el("textarea", { rows: 2, placeholder: "Ask Claude about your project…", maxlength: 8000 });
  private chatForm = el("form", { class: "chat-form" });
  private filesKey = "";

  constructor(
    private root: HTMLElement,
    private onLayout: () => void,
  ) {
    this.chatBox = this.buildChat();
    this.dropBox.append(
      el("div", { class: "title", text: "Drop it on me!" }),
      el("div", { class: "muted small", text: "Then tell me what to do with it." }),
    );
    root.append(this.permBox, this.dropBox, this.menuBox, this.chatBox, this.offerBox, this.noteBox);
  }

  render(v: PetView) {
    this.view = v;
    const show = {
      perm: !!v.permission,
      drop: false,
      menu: false,
      chat: false,
      offer: false,
      note: false,
    };
    if (show.perm) {
      // a question always wins
    } else if (v.dropHover) show.drop = true;
    else if (this.menuOpen) show.menu = true;
    else if (v.chatOpen) show.chat = true;
    else if (v.offer) show.offer = true;
    else show.note = true;

    this.permBox.hidden = !show.perm;
    this.dropBox.hidden = !show.drop;
    this.menuBox.hidden = !show.menu;
    this.chatBox.hidden = !show.chat;
    this.offerBox.hidden = !show.offer;
    this.noteBox.hidden = !show.note;

    if (v.permission) this.renderPermission(v.permission);
    else this.stopCountdown();
    if (show.menu) this.renderMenu(v);
    if (show.chat) this.renderChat(v);
    if (show.offer && v.offer) this.renderOffer(v.offer);
    if (show.note) this.renderNote(v);

    this.root.hidden = false;
    this.root.dataset.tone = show.perm ? "alert" : show.offer || v.toast?.kind === "failed" ? "sick" : "";
    this.onLayout();
  }

  private rerender() {
    if (this.view) this.render(this.view);
  }

  // ---------- quick actions menu (right-click) ----------

  toggleMenu() {
    this.menuOpen = !this.menuOpen;
    this.menuKey = "";
    this.rerender();
  }

  closeMenu() {
    if (!this.menuOpen) return;
    this.menuOpen = false;
    this.rerender();
  }

  private renderMenu(v: PetView) {
    const key = JSON.stringify([v.quickActions, v.emotes]);
    if (key === this.menuKey) return;
    this.menuKey = key;
    const item = (label: string, onClick: () => void, extra = "") =>
      button(label, `menu-item ${extra}`.trim(), () => {
        this.menuOpen = false;
        onClick();
        this.rerender();
      });
    const actions = v.quickActions.map((a) => item(a.label, () => void invoke("run_action", { id: a.id })));
    // Emotes stay in the menu so you can play several in a row.
    const emotes = v.emotes.map((e) => button(e.label, "chip", () => this.onEmote(e.id)));
    this.menuBox.replaceChildren(
      ...compact(
        el("div", { class: "eyebrow", text: "Quick actions" }),
        ...(actions.length ? actions : [el("div", { class: "muted small", text: "No quick actions. Add some in Settings." })]),
        emotes.length ? el("hr") : null,
        emotes.length ? el("div", { class: "eyebrow", text: "Emotes" }) : null,
        emotes.length ? el("div", { class: "emotes" }, ...emotes) : null,
        el("hr"),
        item("Chat with Claude…", () => this.openChat(), "subtle"),
        item("Settings…", () => void invoke("open_settings"), "subtle"),
      ),
    );
  }

  // ---------- chat ----------

  toggleChat() {
    if (this.menuOpen) {
      this.closeMenu();
      return;
    }
    if (this.view?.chatOpen) this.closeChat();
    else this.openChat();
  }

  openChat() {
    // Rust activates the window first; then the text box can take the keyboard.
    diag("click on Glowby: opening chat");
    void invoke("chat_open").finally(() => this.chatInput.focus());
    window.setTimeout(() => {
      this.chatInput.focus();
      diag(`after open: page has focus=${document.hasFocus()}, active element=${document.activeElement?.tagName}`);
    }, 300);
  }

  closeChat() {
    void invoke("chat_close");
  }

  /** The window really lost focus: close the chat if nothing is in progress. */
  onBlur() {
    if (this.picking) return; // the folder picker took focus; keep the chat open
    const v = this.view;
    const idle = v && !v.chat.busy && this.chatInput.value.trim() === "" && v.chat.attachments.length === 0;
    if (v?.chatOpen && idle) this.closeChat();
  }

  /** Glowby slid away: forget the menu. */
  onHidden() {
    this.menuOpen = false;
  }

  private buildChat() {
    const close = button("×", "ghost icon", () => this.closeChat(), "Close (Esc)");
    const stop = button("Stop", "ghost", () => void invoke("chat_cancel"));
    const send = el("button", { class: "btn primary", type: "submit", text: "Send" });
    this.chatActivity.append(el("span", { class: "spinner" }), this.chatActivityText, stop);
    this.chatForm.append(this.chatInput, send);
    this.chatForm.addEventListener("submit", (e) => {
      e.preventDefault();
      void this.send();
    });
    let keysLogged = false;
    this.chatInput.addEventListener("focus", () => {
      keysLogged = false;
      diag(`text box focused (page has focus=${document.hasFocus()})`);
    });
    this.chatInput.addEventListener("keydown", (e) => {
      if (!keysLogged) {
        keysLogged = true;
        diag("key presses are reaching the text box");
      }
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        void this.send();
      } else if (e.key === "Escape") {
        this.closeChat();
      }
    });
    const header = el(
      "div",
      { class: "chat-head" },
      el("strong", { text: "Ask Claude" }),
      this.chatProject,
      el("span", { class: "spacer" }),
      this.chatNewBtn,
      close,
    );
    return el(
      "div",
      { class: "chat" },
      header,
      this.chatNotice,
      this.chatFiles,
      this.chatSuggest,
      this.chatTitle,
      this.chatReply,
      this.chatActivity,
      this.chatError,
      this.chatForm,
    );
  }

  private renderChat(v: PetView) {
    const c = v.chat;
    this.chatProject.textContent = c.hasProject ? `${c.project} ▾` : "Choose folder ▾";
    this.chatProject.title = c.projectPath ? `${c.projectPath}\nClick to choose another folder` : "Choose the project folder";
    this.chatNewBtn.hidden = !c.hasConversation || c.busy;
    this.chatTitle.textContent = c.title ? `You asked: ${c.title}` : "";
    this.chatTitle.hidden = !c.title;
    this.chatReply.textContent = c.reply || (c.busy ? "…" : "");
    this.chatReply.hidden = !c.reply && !c.busy;
    this.chatActivity.hidden = !c.busy;
    this.chatActivityText.textContent = c.activity || "Working…";
    this.chatError.textContent = c.error ?? "";
    this.chatError.hidden = !c.error;

    // Dropped files: chips you can remove, plus one-click suggestions.
    const filesKey = JSON.stringify(c.attachments) + c.busy;
    if (filesKey !== this.filesKey) {
      this.filesKey = filesKey;
      this.chatFiles.replaceChildren(
        ...c.attachments.map((f, i) =>
          el(
            "span",
            { class: "file", title: f.path },
            f.name,
            button("×", "ghost icon tiny", () => void invoke("chat_remove_attachment", { index: i }), "Remove"),
          ),
        ),
      );
      this.chatSuggest.replaceChildren(
        ...FILE_SUGGESTIONS.map(([label, instruction]) => button(label, "", () => void this.sendText(instruction, label))),
      );
    }
    const hasFiles = c.attachments.length > 0;
    this.chatFiles.hidden = !hasFiles;
    this.chatSuggest.hidden = !hasFiles || c.busy;
    this.chatInput.placeholder = hasFiles ? "Or tell me what to do with it…" : "Ask Claude about your project…";

    let notice: (Node | string)[] = [];
    if (!c.enabled) notice = ["Chat is turned off in Settings."];
    else if (!c.hasProject) notice = ["Which project should Claude work in? Click “Choose folder”."];
    else if (c.folderSource === "recent" && !c.reply && !c.busy && !hasFiles)
      notice = [`Using your latest Claude Code project. Click the folder to change it.`];
    this.chatNotice.replaceChildren(...notice);
    this.chatNotice.hidden = notice.length === 0;
    this.chatForm.hidden = !c.enabled;
    this.chatReply.scrollTop = this.chatReply.scrollHeight;
  }

  /** Native folder picker, right from the bubble. */
  private async pickFolder(): Promise<boolean> {
    this.picking = true;
    try {
      const dir = await open({ directory: true, multiple: false, title: "Choose the project folder for chat" });
      if (typeof dir !== "string") return false;
      await invoke("set_chat_folder", { path: dir });
      return true;
    } catch {
      return false;
    } finally {
      this.picking = false;
      this.chatInput.focus();
    }
  }

  private async send() {
    const text = this.chatInput.value.trim();
    if (!text) return;
    if (await this.sendText(text)) this.chatInput.value = "";
  }

  private async sendText(text: string, _label?: string): Promise<boolean> {
    if (this.view?.chat.busy) return false;
    if (!this.view?.chat.hasProject && !(await this.pickFolder())) return false;
    try {
      void invoke("chat_send", { text });
    } catch {
      // The error is part of the next view update and shown in the bubble.
    }
    return true;
  }

  // ---------- offers: copied errors, failing tests ----------

  private renderOffer(o: Offer) {
    const key = JSON.stringify(o);
    if (key === this.offerKey) return;
    this.offerKey = key;
    const choose = (choice: string) => () => void invoke("offer_action", { choice });
    if (o.kind === "break") {
      this.offerBox.replaceChildren(
        el("div", { class: "eyebrow", text: "Glowby" }),
        el("div", { class: "title", text: o.title }),
        el("div", { class: "line", text: `${o.detail} Stretch, drink some water, look at something far away.` }),
        el("div", { class: "actions" }, button("Taking a break", "primary", choose("break")), button("Snooze 15 min", "ghost", choose("snooze"))),
      );
      return;
    }
    const isChecks = o.kind === "failingChecks";
    this.offerBox.replaceChildren(
      ...compact(
        el("div", { class: "eyebrow" }, el("span", { class: "dot failed" }), o.project || "Glowby"),
        el("div", { class: "title", text: o.title }),
        o.detail ? el("pre", { class: "detail small-pre", text: o.detail }) : null,
        el(
          "div",
          { class: "actions" },
          button("Fix it", "primary", choose("fix")),
          button("Explain", "", choose("explain")),
          button(isChecks ? "Dismiss" : "Not now", "ghost", choose("dismiss"), isChecks ? "Stop looking sick until the next failure" : undefined),
        ),
        isChecks ? el("div", { class: "muted small", text: "I'll feel better when they pass." }) : null,
      ),
    );
  }

  // ---------- permission ----------

  private renderPermission(p: PermView) {
    const key = `${p.id}:${p.queued}`;
    if (key === this.permKey) return;
    const isNew = !this.permKey.startsWith(`${p.id}:`);
    this.permKey = key;
    if (isNew) this.deadline = Date.now() + p.expiresInMs;

    const isChat = p.kind === "chatGate";
    const answer = (choice: string) => {
      this.permBox.querySelectorAll("button").forEach((b) => (b.disabled = true));
      void invoke("answer_permission", { id: p.id, choice });
    };
    this.countdown = el("div", { class: "muted small" });
    this.permBox.replaceChildren(
      ...compact(
        el("div", { class: "eyebrow", text: isChat ? `Glowby chat · ${p.project}` : `${p.project || "Claude Code"} · permission` }),
        el("div", { class: "title", text: p.title }),
        p.detail ? el("pre", { class: "detail", text: p.detail }) : null,
        el(
          "div",
          { class: "actions" },
          button("Allow", "primary", () => answer("allow")),
          button("Deny", "", () => answer("deny")),
          isChat ? null : button("In terminal", "ghost", () => answer("terminal"), "Answer in the terminal instead"),
        ),
        this.countdown,
        p.queued > 0 ? el("div", { class: "muted small", text: `+${p.queued} more waiting` }) : null,
      ),
    );
    this.updateCountdown(isChat);
    window.clearInterval(this.countdownTimer);
    this.countdownTimer = window.setInterval(() => this.updateCountdown(isChat), 1000);
  }

  private updateCountdown(isChat: boolean) {
    if (!this.countdown) return;
    const secs = Math.max(0, Math.ceil((this.deadline - Date.now()) / 1000));
    this.countdown.textContent = isChat ? `Not allowed automatically in ${secs}s` : `Goes back to the terminal in ${secs}s`;
  }

  private stopCountdown() {
    window.clearInterval(this.countdownTimer);
    this.countdownTimer = 0;
    this.permKey = "";
  }

  // ---------- notes: toast or status ----------

  private renderNote(v: PetView) {
    let key: string;
    let content: (Node | string | null)[];
    if (v.toast) {
      const t = v.toast;
      key = `toast:${t.kind}:${t.text}:${t.project}`;
      content = [
        el("div", { class: "eyebrow" }, el("span", { class: `dot ${t.kind}` }), t.project || "Glowby"),
        el("div", { class: "line", text: t.text }),
      ];
    } else if (!v.hooksInstalled) {
      key = "nohooks";
      content = [
        el("div", { class: "line", text: "I'm not connected to Claude Code yet." }),
        el("div", { class: "actions" }, button("Connect in Settings", "primary", () => void invoke("open_settings"))),
      ];
    } else if (v.status) {
      const s = v.status;
      key = `status:${s.project}:${s.phase}:${s.activity}:${s.others}`;
      content = [
        el("div", { class: "eyebrow" }, el("span", { class: `dot ${s.phase}` }), `${s.fromPetChat ? "Glowby chat" : s.project} · ${PHASE_LABEL[s.phase] ?? s.phase}`),
        el("div", { class: "line", text: s.activity }),
        s.others > 0 ? el("div", { class: "muted small", text: `+${s.others} other session${s.others > 1 ? "s" : ""}` }) : null,
      ];
    } else {
      key = `quiet:${v.chat.enabled}:${v.quickActions.length}`;
      const hints = [v.chat.enabled ? "click me to chat" : "", v.quickActions.length ? "right-click for quick actions" : ""].filter(Boolean);
      content = [
        el("div", { class: "line", text: "All quiet." }),
        hints.length ? el("div", { class: "muted small", text: `${hints.join(" · ")} · drop a file on me` }) : null,
      ];
    }
    // Level, XP bar and streak under the status (not on toasts).
    const p = v.toast ? null : v.progress;
    key += p ? `|${p.level}:${p.xp}:${p.streak}:${p.energy}` : "";
    if (key === this.noteKey) return;
    this.noteKey = key;
    this.noteBox.replaceChildren(...content.filter((c): c is Node | string => c !== null), ...(p ? [progressLine(p)] : []));
    this.noteBox.onclick = v.toast ? () => void invoke("dismiss_toast") : null;
  }
}

function progressLine(p: ProgressView): HTMLElement {
  const pct = Math.round((p.xpIntoLevel / Math.max(1, p.xpForLevel)) * 100);
  const bits = [`Lv ${p.level} ${p.stageName}`];
  if (p.streak >= 2) bits.push(`${p.streak}-day streak`);
  if (p.energy <= 40) bits.push("tired");
  return el(
    "div",
    { class: "progress", title: `${p.xpIntoLevel} / ${p.xpForLevel} XP to level ${p.level + 1}` },
    el("div", { class: "muted small", text: bits.join(" · ") }),
    el("div", { class: "xpbar" }, el("span", { style: `width:${pct}%` })),
  );
}
