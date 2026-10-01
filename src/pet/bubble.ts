// The speech bubble under Glowby. It shows one thing at a time, by priority:
//   1. a permission question (Allow / Deny)
//   2. the chat box (when you clicked Glowby)
//   3. a short message (task done, needs you, …)
//   4. the live status ("my-app · Editing main.rs")

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { diag } from "../shared/diag";
import { button, compact, el } from "../shared/dom";
import type { PermView, PetView } from "../shared/types";

const PHASE_LABEL: Record<string, string> = {
  idle: "Idle",
  thinking: "Thinking",
  working: "Working",
  needsYou: "Needs you",
  done: "Done",
  failed: "Hit a problem",
};

export class Bubble {
  private view: PetView | null = null;
  private permBox = el("div", { class: "perm" });
  private noteBox = el("div", { class: "note" });
  private chatBox: HTMLElement;
  private permKey = "";
  private noteKey = "";
  private deadline = 0;
  private countdown: HTMLElement | null = null;
  private countdownTimer = 0;

  // chat parts (built once so typing is never interrupted by updates)
  private picking = false;
  private chatProject = button("", "chip", () => void this.pickFolder());
  private chatNewBtn = button("New chat", "ghost", () => void invoke("chat_new"), "Forget this conversation");
  private chatReply = el("div", { class: "reply" });
  private chatActivity = el("div", { class: "activity" });
  private chatActivityText = el("span");
  private chatError = el("div", { class: "error" });
  private chatNotice = el("div", { class: "notice" });
  private chatInput = el("textarea", { rows: 2, placeholder: "Ask Claude about your project…", maxlength: 8000 });
  private chatForm = el("form", { class: "chat-form" });

  constructor(
    private root: HTMLElement,
    private onLayout: () => void,
  ) {
    this.chatBox = this.buildChat();
    root.append(this.permBox, this.chatBox, this.noteBox);
  }

  render(v: PetView) {
    this.view = v;
    const showPerm = !!v.permission;
    const showChat = !showPerm && v.chatOpen;
    this.permBox.hidden = !showPerm;
    this.chatBox.hidden = !showChat;
    this.noteBox.hidden = showPerm || showChat;

    if (v.permission) this.renderPermission(v.permission);
    else this.stopCountdown();
    if (showChat) this.renderChat(v);
    if (!showPerm && !showChat) this.renderNote(v);

    this.root.hidden = false;
    this.root.dataset.tone = showPerm ? "alert" : v.toast?.kind === "failed" ? "sick" : "";
    this.onLayout();
  }

  toggleChat() {
    if (this.view?.chatOpen) this.closeChat();
    else this.openChat();
  }

  openChat() {
    // Rust activates the window first; then the text box can take the keyboard.
    diag("click on Glowby: opening chat");
    void invoke("chat_open").finally(() => this.chatInput.focus());
    window.setTimeout(() => {
      this.chatInput.focus();
      diag(`after open: page has focus=${document.hasFocus()}, active element=${document.activeElement?.tagName}, form hidden=${this.chatForm.hidden}`);
    }, 300);
  }

  closeChat() {
    void invoke("chat_close");
  }

  /** The window lost focus: close the chat if nothing is in progress. */
  onBlur() {
    if (this.picking) return; // the folder picker took focus; keep the chat open
    const v = this.view;
    if (v?.chatOpen && !v.chat.busy && this.chatInput.value.trim() === "") this.closeChat();
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
    this.permBox.replaceChildren(...compact(
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
    ));
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

  // ---------- chat ----------

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
    this.chatInput.addEventListener("blur", () => diag("text box lost focus"));
    this.chatInput.addEventListener("pointerdown", () => diag("pointer down on text box"));
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
    const header = el("div", { class: "chat-head" }, el("strong", { text: "Ask Claude" }), this.chatProject, el("span", { class: "spacer" }), this.chatNewBtn, close);
    return el("div", { class: "chat" }, header, this.chatNotice, this.chatReply, this.chatActivity, this.chatError, this.chatForm);
  }

  private renderChat(v: PetView) {
    const c = v.chat;
    this.chatProject.textContent = c.hasProject ? `${c.project} ▾` : "Choose folder ▾";
    this.chatProject.title = c.projectPath ? `${c.projectPath}\nClick to choose another folder` : "Choose the project folder";
    this.chatNewBtn.hidden = !c.hasConversation || c.busy;
    this.chatReply.textContent = c.reply || (c.busy ? "…" : "");
    this.chatReply.hidden = !c.reply && !c.busy;
    this.chatActivity.hidden = !c.busy;
    this.chatActivityText.textContent = c.activity || "Working…";
    this.chatError.textContent = c.error ?? "";
    this.chatError.hidden = !c.error;

    let notice: (Node | string)[] = [];
    if (!c.enabled) notice = ["Chat is turned off in Settings."];
    else if (!c.hasProject) notice = ["Which project should Claude work in? Click “Choose folder”."];
    else if (c.folderSource === "recent" && !c.reply && !c.busy)
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
    if (!text || this.view?.chat.busy) return;
    if (!this.view?.chat.hasProject && !(await this.pickFolder())) return;
    this.chatInput.value = "";
    try {
      await invoke("chat_send", { text });
    } catch {
      // The error is part of the next view update and shown in the bubble.
    }
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
      key = `quiet:${v.chat.enabled}`;
      content = [
        el("div", { class: "line", text: "All quiet." }),
        v.chat.enabled ? el("div", { class: "muted small", text: "Click me to chat · drag me sideways to move" }) : null,
      ];
    }
    if (key === this.noteKey) return;
    this.noteKey = key;
    this.noteBox.replaceChildren(...content.filter((c): c is Node | string => c !== null));
    this.noteBox.onclick = v.toast ? () => void invoke("dismiss_toast") : null;
  }
}
