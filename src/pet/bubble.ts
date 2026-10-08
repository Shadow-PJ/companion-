// The speech bubble under Glowby. It shows one thing at a time, by priority:
//   1. a permission question (Allow / Deny)
//   2. "drop it on me" while you drag a file over Glowby
//   3. the quick-actions menu (right-click)
//   3b. a squad pet's card (you clicked one of the small pets)
//   4. the chat box (when you clicked Glowby or dropped a file)
//   5. an offer ("That looks like an error. Fix it?", failing tests)
//   6. a short message (task done, needs you, …)
//   7. the live status ("my-app · Editing main.rs")

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { diag } from "../shared/diag";
import { button, compact, el } from "../shared/dom";
import type { BriefingView, CaseReport, LimitsView, Offer, PermView, PetView, ProgressView, QuestView, QuizView, SquadMember } from "../shared/types";

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
  onInteraction: (kind: string) => void = () => {};
  /** Set by main.ts: the selected squad pet changed (it gets a spotlight). */
  onSquadChange: () => void = () => {};
  /** The squad pet whose card is open, if any. */
  squadSelected: string | null = null;
  private view: PetView | null = null;
  private replyKey = "";
  private replyOpen = false;
  private replyDetails: HTMLDetailsElement | null = null;
  private menuOpen = false;
  /** The AI limits card is open (right-click → AI limits). */
  private limitsOpen = false;
  private limitsBox = el("div", { class: "limits-card" });
  private limitsKey = "";
  private caseBox = el("div", { class: "case-card" });
  private caseKey = "";
  private squadBox = el("div", { class: "squad-card" });
  private squadKey = "";
  private squadError = "";
  private permBox = el("div", { class: "perm" });
  private dropBox = el("div", { class: "note drop" });
  private menuBox = el("div", { class: "menu" });
  private offerBox = el("div", { class: "offer" });
  private quizBox = el("div", { class: "quiz" });
  private briefBox = el("div", { class: "brief" });
  private noteBox = el("div", { class: "note" });
  private quizKey = "";
  private briefKey = "";
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
  private chatInput = el("textarea", { rows: 2, placeholder: "Ask your agent about your project…", maxlength: 8000 });
  private chatHeading = el("strong", { text: "Ask your agent" });
  private agentSelect = el("select", { class: "agent-select", "aria-label": "Chat provider" },
    el("option", { value: "auto", text: "Auto" }), el("option", { value: "claude", text: "Claude" }), el("option", { value: "codex", text: "Codex" }));
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
    root.append(this.permBox, this.dropBox, this.menuBox, this.limitsBox, this.squadBox, this.chatBox, this.caseBox, this.quizBox, this.briefBox, this.offerBox, this.noteBox);
  }

  render(v: PetView) {
    this.view = v;
    const squadPet = v.squad.find((m) => m.id === this.squadSelected) ?? null;
    if (this.squadSelected && !squadPet) {
      this.squadSelected = null; // that session ended
      this.onSquadChange();
    }
    const show = {
      perm: !!v.permission,
      drop: false,
      menu: false,
      limits: false,
      squad: false,
      chat: false,
      case: false,
      quiz: false,
      brief: false,
      offer: false,
      note: false,
    };
    if (show.perm) {
      // a question always wins
    } else if (v.dropHover) show.drop = true;
    else if (this.menuOpen) show.menu = true;
    else if (this.limitsOpen && v.limits) show.limits = true;
    else if (squadPet) show.squad = true;
    else if (v.chatOpen) show.chat = true;
    else if (v.caseReport) show.case = true;
    else if (v.quiz) show.quiz = true;
    else if (v.briefing) show.brief = true;
    else if (v.offer) show.offer = true;
    else show.note = true;

    this.permBox.hidden = !show.perm;
    this.dropBox.hidden = !show.drop;
    this.menuBox.hidden = !show.menu;
    this.limitsBox.hidden = !show.limits;
    this.squadBox.hidden = !show.squad;
    this.chatBox.hidden = !show.chat;
    this.caseBox.hidden = !show.case;
    this.quizBox.hidden = !show.quiz;
    this.briefBox.hidden = !show.brief;
    this.offerBox.hidden = !show.offer;
    this.noteBox.hidden = !show.note;

    if (v.permission) this.renderPermission(v.permission);
    else this.stopCountdown();
    if (show.menu) this.renderMenu(v);
    if (show.limits && v.limits) this.renderLimits(v.limits);
    if (show.squad && squadPet) this.renderSquad(squadPet, v);
    if (show.chat) this.renderChat(v);
    if (show.case && v.caseReport) this.renderCase(v.caseReport);
    if (show.quiz && v.quiz) this.renderQuiz(v.quiz);
    if (show.brief && v.briefing) this.renderBriefing(v.briefing, v.quests);
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
    const key = JSON.stringify([v.quickActions, v.emotes, v.autoAllow, !!v.limits, v.chat.agent]);
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
        el("div", { class: "eyebrow", text: "Auto-allow questions (Claude & Codex)" }),
        this.autoAllowControls(v),
        el("hr"),
        v.limits ? item("AI limits (Claude & Codex)", () => this.openLimits(), "subtle") : null,
        item("AI Pulse · news, models and tools", () => void invoke("pulse_open").catch(e => { this.chatError.textContent = String(e); this.chatError.hidden = false; }), "subtle"),
        item("Token detective: case report", () => void invoke("detective_run"), "subtle"),
        item(`Chat with ${v.chat.agent === "codex" ? "Codex" : "Claude"}…`, () => this.openChat(), "subtle"),
        item("Today's briefing and quests", () => void invoke("briefing_show"), "subtle"),
        item("Settings…", () => void invoke("open_settings"), "subtle"),
      ),
    );
  }

  // ---------- token detective ----------

  private renderCase(c: CaseReport) {
    const key = JSON.stringify([c, this.view?.chat.agent]);
    if (key === this.caseKey) return;
    this.caseKey = key;
    const findings = c.findings.map((f, i) =>
      el(
        "div",
        { class: "case-finding" },
        el("div", { class: "limit-head" }, el("strong", { text: `${i + 1}. ${f.title}` }), el("span", { class: "muted small", text: f.share })),
        el("div", { class: "muted small", text: f.detail }),
        el("div", { class: "line small", text: `Tip: ${f.tip}` }),
        f.canFix ? el("div", { class: "actions" }, button(`Ask ${this.view?.chat.agent === "codex" ? "Codex" : "Claude"} to fix it`, "", () => void invoke("detective_fix", { index: i }))) : null,
      ),
    );
    this.caseBox.replaceChildren(
      ...compact(
        el("div", { class: "eyebrow", text: `🔍 Token detective · case report${c.period ? ` · ${c.period}` : ""}` }),
        c.running ? el("div", { class: "activity" }, el("span", { class: "spinner" }), el("span", { text: "Investigating your logs…" })) : null,
        c.summary && !c.running ? el("div", { class: "muted small", text: c.summary }) : null,
        ...(c.running ? [] : findings.length ? findings : [el("div", { class: "line", text: "No big leaks found this week. Nice work!" })]),
        c.footnote && !c.running ? el("div", { class: "muted small", text: c.footnote }) : null,
        el("div", { class: "actions" }, button("Close case", "ghost", () => void invoke("detective_close"))),
      ),
    );
  }

  // ---------- AI limits ----------

  openLimits() {
    this.limitsOpen = true;
    this.menuOpen = false;
    this.limitsKey = "";
    void invoke("limits_refresh"); // fresh numbers arrive with the next update
    this.rerender();
  }

  private renderLimits(l: LimitsView) {
    const key = JSON.stringify(l);
    if (key === this.limitsKey) return;
    this.limitsKey = key;
    const agents = l.agents.map((a) =>
      el(
        "div",
        { class: "limits-agent" },
        el("div", { class: "title", text: a.plan ? `${a.agent} · ${a.plan} plan` : a.agent }),
        ...(a.windows.length
          ? a.windows.map((w) =>
              el(
                "div",
                { class: "limit" + (w.tight ? " tight" : "") + (w.stale ? " stale" : "") },
                el("div", { class: "limit-head" }, el("span", { text: `${w.label} limit` }), el("strong", { text: w.usedText })),
                w.used !== null ? el("div", { class: "xpbar" }, el("span", { style: `width:${Math.min(100, Math.max(0, w.used))}%` })) : null,
                el("div", { class: "muted small", text: `${w.resetsText} · ${w.forecast}` }),
                el("div", { class: "muted small", text: `${w.exact ? "" : "Estimate · "}${w.asOf}${w.stale ? " (may be out of date)" : ""}` }),
              ),
            )
          : [el("div", { class: "muted small", text: a.emptyHint })]),
      ),
    );
    this.limitsBox.replaceChildren(
      el("div", { class: "eyebrow", text: "AI limits" }),
      ...agents,
      el("div", { class: "muted small", text: "Numbers can be a few minutes old. “Runs low in” is a rough guess from your recent pace." }),
      el(
        "div",
        { class: "actions" },
        button("Refresh", "", () => void invoke("limits_refresh")),
        button("Close", "ghost", () => {
          this.limitsOpen = false;
          this.rerender();
        }),
      ),
    );
  }

  /** Chips to start timed auto-allow, or its state and a Stop button. */
  private autoAllowControls(v: PetView): HTMLElement {
    const a = v.autoAllow;
    const chip = (label: string, onClick: () => void, extra = "") =>
      button(label, `chip ${extra}`.trim(), () => {
        this.menuOpen = false;
        onClick();
        this.rerender();
      });
    if (a?.full) return el("div", { class: "muted small", text: "Full auto is on (turn it off in Settings)." });
    if (a) {
      return el(
        "div",
        { class: "emotes" },
        el("span", { class: "muted small", text: `On · ${a.minutesLeft} min left · ` }),
        chip("Stop", () => void invoke("auto_allow_stop"), "primary"),
      );
    }
    return el(
      "div",
      { class: "emotes" },
      ...([15, 30, 60] as const).map((m) => chip(m === 60 ? "1 hour" : `${m} min`, () => void invoke("auto_allow_start", { minutes: m }))),
    );
  }

  // ---------- chat ----------

  toggleChat() {
    if (this.menuOpen) {
      this.closeMenu();
      return;
    }
    if (this.squadSelected) {
      this.closeSquad();
      return;
    }
    if (this.limitsOpen) {
      this.limitsOpen = false;
      this.rerender();
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

  /** Glowby slid away: forget the menu and the squad card. */
  onHidden() {
    this.menuOpen = false;
    this.limitsOpen = false;
    if (this.squadSelected) {
      this.squadSelected = null;
      this.onSquadChange();
    }
  }

  // ---------- squad pets ----------

  toggleSquad(id: string) {
    this.squadSelected = this.squadSelected === id ? null : id;
    this.squadError = "";
    this.menuOpen = false;
    this.squadKey = "";
    this.onSquadChange();
    this.rerender();
  }

  private closeSquad() {
    this.squadSelected = null;
    this.onSquadChange();
    this.rerender();
  }

  private renderSquad(m: SquadMember, v: PetView) {
    const key = JSON.stringify([m, v.characters, v.pets, v.chat.enabled, this.squadError]);
    if (key === this.squadKey) return;
    this.squadKey = key;
    const pct = Math.round((m.xpIntoLevel / Math.max(1, m.xpForLevel)) * 100);
    const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
    const current = m.character ? `c:${m.character}` : m.species ? `p:${m.species}` : "";
    const lookChip = (key: string, label: string, character: string, species: string) =>
      button(label, current === key ? "chip primary" : "chip", () => void invoke("squad_set_look", { id: m.id, character, species }));
    const chat = button(`Chat with ${m.name}`, "primary", () => {
      this.squadSelected = null;
      this.onSquadChange();
      void invoke("squad_chat_open", { id: m.id })
        .then(() => this.chatInput.focus())
        .catch((e) => {
          // e.g. the session's folder is gone: show why on the card
          this.squadSelected = m.id;
          this.squadError = String(e);
          this.onSquadChange();
          this.rerender();
        });
    });
    chat.disabled = !v.chat.enabled;
    this.squadBox.replaceChildren(
      ...compact(
        el("div", { class: "eyebrow" }, el("span", { class: `dot ${m.phase}` }), `Squad · ${m.project || "Agent session"}`),
        el("div", { class: "title", text: `${m.name} · Level ${m.level}` }),
        el("div", { class: "line", text: m.activity }),
        el("div", {
          class: "muted small",
          text: `${m.minutes < 1 ? "Just started" : `Running ${m.minutes} min`} · ${plural(m.tasks, "task")} · ${plural(m.tools, "tool use")}`,
        }),
        el("div", { class: "xpbar", title: `${m.xpIntoLevel} / ${m.xpForLevel} XP to level ${m.level + 1}` }, el("span", { style: `width:${pct}%` })),
        el("div", { class: "eyebrow", text: "Look" }),
        el(
          "div",
          { class: "looks" },
          lookChip("", "Jellyfish", "", ""),
          ...v.pets.map((p) => lookChip(`p:${p.id}`, p.label, "", p.id)),
          ...v.characters.map((c) => lookChip(`c:${c.id}`, c.label, c.id, "")),
        ),
        v.characters.length ? null : el("div", { class: "muted small", text: "Import characters in Settings to dress up your squad." }),
        this.squadError ? el("div", { class: "error", text: this.squadError }) : null,
        el("div", { class: "actions" }, chat, button("Close", "ghost", () => this.closeSquad())),
        el("div", { class: "muted small", text: "Chat works in this session's folder on a copy of its conversation, so your terminal isn't disturbed." }),
      ),
    );
  }


  private buildChat() {
    this.agentSelect.addEventListener("change", () => {
      void invoke("chat_select_agent", { agent: this.agentSelect.value, open: false }).catch(e => { this.chatError.textContent=String(e);this.chatError.hidden=false; });
    });
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
      this.chatHeading,
      this.chatProject,
      el("span", { class: "spacer" }),
      this.chatNewBtn,
      close,
    );
    return el(
      "div",
      { class: "chat" },
      header,
      el("div", {class:"agent-choice"}, el("span", {text:"Provider"}), this.agentSelect, el("small", {text:"Uses this agent’s account"})),
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
    const agent = c.agent === "codex" ? "Codex" : "Claude";
    this.agentSelect.value = c.agentMode;
    this.agentSelect.disabled = c.busy || !!c.squadName;
    this.chatHeading.textContent = c.squadName ? `Ask ${c.squadName} · ${agent}` : `Ask ${agent}`;
    this.chatProject.textContent = c.hasProject ? `${c.project} ▾` : "Choose folder ▾";
    this.chatProject.title = c.squadName
      ? `${c.projectPath}\nClick to choose a folder (back to Glowby's own chat)`
      : c.projectPath
        ? `${c.projectPath}\nClick to choose another folder`
        : "Choose the project folder";
    this.chatNewBtn.hidden = !c.hasConversation || c.busy;
    const replyAgent = c.replyAgent === "codex" ? "Codex" : "Claude";
    this.chatTitle.textContent = c.title ? `${c.replyAgent ? `${replyAgent} · ` : ""}You asked: ${c.title}` : "";
    this.chatTitle.hidden = !c.title;
    const replyText = c.reply || (c.busy ? "…" : "");
    const followReply = this.chatReply.scrollHeight - this.chatReply.clientHeight - this.chatReply.scrollTop < 32;
    const replyChanged = this.chatReply.textContent !== replyText;
    if (replyChanged) this.chatReply.textContent = replyText;
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
    this.chatInput.placeholder = hasFiles ? "Or tell me what to do with it…" : `Ask ${agent} about your project…`;

    let notice: (Node | string)[] = [];
    if (!c.enabled) notice = ["Chat is turned off in Settings."];
    else if (!c.hasProject) notice = ["Start a Claude Code or Codex session once and I'll use its folder, or click “Choose folder”."];
    else if (c.folderSource === "squad" && !c.reply && !c.busy)
      notice = [`Chatting in ${c.squadName}'s project. A separate copy of that agent's conversation keeps the original session undisturbed.`];
    else if (c.folderSource === "recent" && !c.reply && !c.busy && !hasFiles)
      notice = [`Working in the folder your Claude Code or Codex sessions used last. Click the folder to change it.`];
    this.chatNotice.replaceChildren(...notice);
    this.chatNotice.hidden = notice.length === 0;
    this.chatForm.hidden = !c.enabled;
    if (replyChanged && followReply) this.chatReply.scrollTop = this.chatReply.scrollHeight;
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

  // ---------- learn mode ----------

  private renderQuiz(q: QuizView) {
    const key = JSON.stringify(q);
    if (key === this.quizKey) return;
    this.quizKey = key;
    const r = q.result;
    const options = q.options.map((text, i) => {
      let cls = "menu-item option";
      if (r && i === r.correct) cls += " right";
      else if (r && i === r.chosen) cls += " wrong";
      const b = button(`${String.fromCharCode(65 + i)}. ${text}`, cls, () => void invoke("quiz_answer", { index: i }));
      b.disabled = !!r;
      return b;
    });
    this.quizBox.replaceChildren(
      ...compact(
        el("div", { class: "eyebrow", text: `Learn mode${q.project ? ` · ${q.project}` : ""}` }),
        el("div", { class: "title", text: q.question }),
        ...options,
        r ? el("div", { class: r.right ? "result right" : "result wrong", text: r.right ? "Right! +15 XP" : "Not quite." }) : null,
        r && r.explain ? el("div", { class: "muted small", text: r.explain }) : null,
        r ? null : el("div", { class: "actions" }, button("Skip", "ghost", () => void invoke("quiz_skip"))),
      ),
    );
  }

  // ---------- daily briefing ----------

  private renderBriefing(b: BriefingView, quests: QuestView[]) {
    const key = JSON.stringify([b, quests]);
    if (key === this.briefKey) return;
    this.briefKey = key;
    const list = (items: string[]) => el("ul", { class: "list" }, ...items.map((t) => el("li", { text: t })));
    this.briefBox.replaceChildren(
      ...compact(
        el("div", { class: "title", text: b.greeting }),
        b.done.length ? el("div", { class: "eyebrow", text: b.period }) : null,
        b.done.length ? list(b.done) : el("div", { class: "muted small", text: "No commits from you recently in the projects I know." }),
        b.unfinished.length ? el("div", { class: "eyebrow", text: "Unfinished" }) : null,
        b.unfinished.length ? list(b.unfinished) : null,
        quests.length ? el("div", { class: "eyebrow", text: "Today's quests" }) : null,
        quests.length ? questList(quests) : null,
        b.suggestion ? el("div", { class: "eyebrow", text: "One small next step" }) : null,
        b.suggestion ? el("div", { class: "line", text: b.suggestion.text }) : null,
        el(
          "div",
          { class: "actions" },
          ...compact(
            b.suggestion ? button(`Do it with ${this.view?.chat.agent === "codex" ? "Codex" : "Claude"}`, "primary", () => void invoke("briefing_do")) : null,
            button("Thanks!", b.suggestion ? "ghost" : "primary", () => void invoke("briefing_dismiss")),
          ),
        ),
      ),
    );
  }

  // ---------- offers: copied errors, failing tests, CI, breaks ----------

  private renderOffer(o: Offer) {
    const key = JSON.stringify(o);
    if (key === this.offerKey) return;
    this.offerKey = key;
    const choose = (choice: string) => () => void invoke("offer_action", { choice });
    if (o.kind === "ci") {
      this.offerBox.replaceChildren(
        el("div", { class: "eyebrow" }, el("span", { class: "dot failed" }), `${o.project} · GitHub Actions`),
        el("div", { class: "title", text: o.title }),
        el("div", { class: "muted small", text: o.detail }),
        el(
          "div",
          { class: "actions" },
          button("Open on GitHub", "", choose("open")),
          button("Fix it", "primary", choose("fix")),
          button("Dismiss", "ghost", choose("dismiss")),
        ),
      );
      return;
    }
    if (o.kind === "limits") {
      this.offerBox.replaceChildren(
        el("div", { class: "eyebrow" }, el("span", { class: "dot attention" }), "AI limits"),
        el("div", { class: "title", text: o.title }),
        el("div", { class: "line", text: o.detail }),
        el(
          "div",
          { class: "actions" },
          button("Copy handoff note", "primary", choose("copyNote"), "A note with what you were doing, to paste into a new chat or the other agent (made on your PC, no AI)"),
          button("Show limits", "", () => {
            void invoke("offer_action", { choice: "dismiss" });
            this.openLimits();
          }),
          button("OK", "ghost", choose("dismiss")),
        ),
      );
      return;
    }
    if (o.kind === "guard" || o.kind === "limitHit" || o.kind === "claudeBack" || o.kind === "cacheSoon" || o.kind === "bigChat") {
      this.renderUsageAlert(o, choose);
      return;
    }
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

  /** The alerts that save your usage: a stopped message, a limit hit, a cold cache, a big chat. */
  private renderUsageAlert(o: Offer, choose: (choice: string) => () => void) {
    const eyebrow = {
      guard: "Saved your usage",
      limitHit: "AI limits",
      claudeBack: "AI limits",
      cacheSoon: "Token detective",
      bigChat: "Token detective",
    }[o.kind as "guard" | "limitHit" | "claudeBack" | "cacheSoon" | "bigChat"];
    const dot = o.kind === "claudeBack" ? "done" : "attention";
    const copy = (label: string) => button(label, "primary", choose("copyNote"), "A note with what you were doing, to paste into a new chat (made on your PC, no AI)");
    const buttons =
      o.kind === "claudeBack"
        ? [button("Yay!", "primary", choose("dismiss"))]
        : o.kind === "limitHit"
          ? [
              copy("Copy handoff note"),
              button("Show limits", "", () => {
                void invoke("offer_action", { choice: "dismiss" });
                this.openLimits();
              }),
              button("OK", "ghost", choose("dismiss")),
            ]
          : [copy(o.kind === "guard" ? "Copy note again" : "Copy fresh-start note"), button(o.kind === "bigChat" ? "Not now" : "OK", "ghost", choose("dismiss"))];
    this.offerBox.replaceChildren(
      el("div", { class: "eyebrow" }, el("span", { class: `dot ${dot}` }), o.project ? `${eyebrow} · ${o.project}` : eyebrow),
      el("div", { class: "title", text: o.title }),
      el("div", { class: "line", text: o.detail }),
      el("div", { class: "actions" }, ...buttons),
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
      void invoke("answer_permission", { id: p.id, choice }).catch(error => {
        this.permBox.querySelectorAll("button").forEach(b => b.disabled = false);
        this.countdown!.textContent = String(error);
        this.countdown!.setAttribute("role", "alert");
        window.clearInterval(this.countdownTimer);
      });
    };
    this.countdown = el("div", { class: "muted small" });
    this.permBox.replaceChildren(
      ...compact(
        el("div", {
          class: "eyebrow",
          text: isChat ? `Glowby chat · ${p.project}` : `${p.agent === "codex" ? "Codex" : "Claude Code"}${p.project ? ` · ${p.project}` : ""} · permission`,
        }),
        el("div", { class: "title", text: p.title }),
        p.detail ? el("pre", { class: "detail", text: p.detail }) : null,
        el(
          "div",
          { class: "actions" },
          button("Allow", "primary", () => answer("allow")),
          button("Deny", "", () => answer("deny")),
          isChat ? null : p.agent === "codex" ? button("In Codex", "ghost", () => answer("terminal"), "Answer in Codex instead") : button("In terminal", "ghost", () => answer("terminal"), "Answer in the terminal instead"),
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
    this.countdown.textContent = isChat ? `Not allowed automatically in ${secs}s` : `Goes back to ${this.view?.permission?.agent === "codex" ? "Codex" : "the terminal"} in ${secs}s`;
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
        el("div", { class: "line", text: "Connect Claude Code or Codex in Settings." }),
        el("div", { class: "actions" }, button("Connect in Settings", "primary", () => void invoke("open_settings"))),
      ];
    } else if (v.status) {
      const s = v.status;
      key = `status:${s.agent}:${s.project}:${s.phase}:${s.activity}:${s.others}`;
      content = [
        el("div", { class: "eyebrow" }, el("span", { class: `dot ${s.phase}` }), `${s.fromPetChat ? "Glowby chat" : s.agent === "codex" ? "Codex" : "Claude"}${s.project ? ` · ${s.project}` : ""} · ${PHASE_LABEL[s.phase] ?? s.phase}`),
        el("div", { class: "line", text: s.activity }),
        s.others > 0 ? el("div", { class: "muted small", text: `+${s.others} other session${s.others > 1 ? "s" : ""}` }) : null,
      ];
    } else {
      key = `quiet:${v.chat.agent}:${v.chat.agentMode}:${v.chat.enabled}:${v.interactions}`;
      const hints = ["Stroke my head to pet me", "right-click for more"].filter(Boolean);
      content = [
        el("div", { class: "line", text: `${v.chat.agent === "codex" ? "Codex" : "Claude"} is ready.` }),
        hints.length ? el("div", { class: "muted small", text: hints.join(" · ") }) : null,
      ];
    }
    // Answers from the user's Code/desktop sessions are separate from pet chat.
    // Native details keeps reading selectable text free of extra windows or timers.
    const reply = v.agentReply;
    if (reply) {
      key += `|reply:${reply.agent}:${reply.sessionId}:${reply.asOf}:${reply.text}`;
      const replyKey = `${reply.agent}:${reply.sessionId}:${reply.asOf}:${reply.text}`;
      if (replyKey !== this.replyKey || !this.replyDetails) {
        this.replyKey = replyKey;
        this.replyOpen = v.status?.phase === "done" || v.toast?.kind === "done";
        const label = `${reply.agent === "codex" ? "Codex" : "Claude"} reply${reply.project ? ` · ${reply.project}` : ""}`;
        const details = el("details", { class: "agent-reply" },
          el("summary", { text: label }),
          el("div", { class: "muted small", text: reply.asOf }),
          el("div", { class: "reply", text: reply.text }));
        details.addEventListener("toggle", () => {
          if (!details.isConnected) return;
          this.replyOpen = details.open;
          this.onLayout();
        });
        this.replyDetails = details;
      }
      const details = this.replyDetails;
      details.open = this.replyOpen;
      content.push(details);
    } else {
      this.replyDetails = null;
      this.replyKey = "";
    }
    // Auto-allow, quietly: one line with a Stop button (timed) under the status.
    const a = v.autoAllow;
    if (a && !v.toast) {
      key += `|auto:${a.full}:${a.minutesLeft}:${a.allowed}:${a.note ?? ""}`;
      const text = `${a.full ? "Auto-allow on" : `Auto-allow · ${a.minutesLeft} min`} ${a.allowed ? `· ${a.allowed} answered` : ""}`;
      content.push(
        el(
          "div",
          { class: "auto-line" },
          el("span", { text }),
          a.full ? null : button("Stop", "ghost tiny", () => void invoke("auto_allow_stop"), "Turn auto-allow off"),
        ),
      );
      if (a.note) content.push(el("div", { class: "muted small", text: a.note }));
    }
    // Both providers stay discoverable. Missing numbers are never fabricated.
    if (!v.toast && v.limits) {
      const cards = ["Claude", "Codex"].map(name => {
        const agent = v.limits!.agents.find(a => a.agent === name), window = agent?.windows[0];
        const active = (v.chat.agent === "codex" ? "Codex" : "Claude") === name;
        key += `|usage:${name}:${active}:${window?.usedText}:${window?.resetsText}`;
        const card = button("", `usage-chip ${active ? "active" : ""}`, () => this.openLimits());
        card.title = window ? `${window.label} · ${window.resetsText} · ${window.asOf}${window.stale ? " · old reading" : ""}` : agent?.emptyHint ?? "No usage reading yet.";
        card.append(el("span", {text:name}), el("strong", {text:window?.usedText ?? "No reading yet"}));
        if(window?.used!==null && window?.used!==undefined)card.append(el("span",{class:"usage-track"},el("i",{style:`width:${Math.max(0,Math.min(100,window.used))}%`})));
        return card;
      });
      content.push(el("div", {class:"usage-pair"}, ...cards));
    }
    if (!v.toast && v.interactions) {
      key += `|interactions:${v.emotes.map(e=>e.id).join(",")}`;
      const agent = v.chat.agent === "codex" ? "Codex" : "Claude";
      content.push(el("div", {class:"pet-controls", "aria-label":"Pet interactions"},
        button(`Ask ${agent}`, "primary tiny", () => this.openChat()),
        button("Wave", "ghost tiny", () => this.onInteraction("wave")),
        button("Pet", "ghost tiny", () => this.onInteraction("pet")),
        button("Play", "ghost tiny", () => { const choices=v.emotes.map(e=>e.id);this.onInteraction(choices[Math.floor(Math.random()*choices.length)] ?? "wave"); }, "Play an unlocked emote")));
    }
    // Level, XP bar, streak and quests under the status (not on toasts).
    const p = v.toast ? null : v.progress;
    const questsDone = v.quests.filter((q) => q.done).length;
    key += p ? `|${p.level}:${p.xp}:${p.streak}:${p.energy}:${questsDone}/${v.quests.length}` : "";
    if (key === this.noteKey) return;
    this.noteKey = key;
    this.noteBox.replaceChildren(
      ...content.filter((c): c is Node | string => c !== null),
      ...(p ? [progressLine(p, v.quests.length ? `Quests ${questsDone}/${v.quests.length}` : "")] : []),
    );
    this.noteBox.onclick = v.toast ? (event) => {
      if (event.target instanceof Element && event.target.closest("details,.btn")) return;
      void invoke("dismiss_toast");
    } : null;
  }
}

function questList(quests: QuestView[]): HTMLElement {
  return el(
    "ul",
    { class: "list quests" },
    ...quests.map((q) =>
      el(
        "li",
        { class: q.done ? "done" : "" },
        `${q.done ? "✓" : "○"} ${q.label}`,
        el("span", { class: "muted small", text: q.done ? ` +${q.xp} XP` : ` ${q.progress}/${q.target}` }),
      ),
    ),
  );
}

function progressLine(p: ProgressView, quests: string): HTMLElement {
  const pct = Math.round((p.xpIntoLevel / Math.max(1, p.xpForLevel)) * 100);
  const bits = [`Lv ${p.level} ${p.stageName}`];
  if (p.streak >= 2) bits.push(`${p.streak}-day streak`);
  if (quests) bits.push(quests);
  if (p.energy <= 40) bits.push("tired");
  return el(
    "div",
    { class: "progress", title: `${p.xpIntoLevel} / ${p.xpForLevel} XP to level ${p.level + 1}` },
    el("div", { class: "muted small", text: bits.join(" · ") }),
    el("div", { class: "xpbar" }, el("span", { style: `width:${pct}%` })),
  );
}
