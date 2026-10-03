// Mirrors of the Rust structs sent to the web pages (keep in sync with src-tauri).

import type { Mood } from "../pet/character/palette";
export type { Mood };

export interface StatusView {
  project: string;
  activity: string;
  phase: "idle" | "thinking" | "working" | "needsYou" | "done" | "failed";
  fromPetChat: boolean;
  others: number;
}

export interface PermView {
  id: number;
  kind: "permission" | "chatGate";
  agent: "claude" | "codex";
  project: string;
  title: string;
  detail: string;
  expiresInMs: number;
  queued: number;
}

export interface Toast {
  kind: "done" | "attention" | "failed" | "info" | "levelup";
  text: string;
  project: string;
}

export interface ProgressView {
  level: number;
  stage: number;
  stageName: string;
  xp: number;
  xpIntoLevel: number;
  xpForLevel: number;
  streak: number;
  energy: number;
}

export interface Look {
  stage: number;
  hat: string;
  color: string;
  weak: boolean;
  /** Power-up effect around Glowby ("" = none). */
  aura: string;
  /** Imported character id ("" = none). Wins over `species`. */
  character: string;
  /** Anime pet id ("" = Glowby the jellyfish). */
  species: string;
}

export interface CosmeticView {
  id: string;
  kind: "hat" | "color" | "emote" | "aura" | "pet";
  name: string;
  unlocked: boolean;
  requirement: string;
}

export interface ProgressInfo {
  view: ProgressView;
  look: Look;
  stats: { tasks: number; fixes: number; testsPassed: number; commits: number; breaks: number; pets?: number };
  bestStreak: number;
  stages: [number, string][];
  cosmetics: CosmeticView[];
}

export interface Offer {
  kind: "clipboardError" | "failingChecks" | "break" | "ci" | "limits";
  title: string;
  detail: string;
  project: string;
  url: string | null;
}

export interface QuestView {
  kind: string;
  label: string;
  progress: number;
  target: number;
  done: boolean;
  xp: number;
}

export interface BriefingView {
  greeting: string;
  period: string;
  done: string[];
  unfinished: string[];
  suggestion: { text: string } | null;
}

export interface QuizView {
  question: string;
  options: string[];
  project: string;
  result: { chosen: number; correct: number; right: boolean; explain: string } | null;
}

export interface RepoStatus {
  repo: string;
  branch: string;
  state: string;
  url: string;
  checked: string;
}

export interface GithubStatus {
  hasToken: boolean;
  repos: RepoStatus[];
}

export interface ActionView {
  id: string;
  label: string;
}

export interface Attachment {
  name: string;
  path: string;
}

export interface ChatView {
  enabled: boolean;
  busy: boolean;
  reply: string;
  activity: string;
  error: string | null;
  title: string;
  attachments: Attachment[];
  project: string;
  projectPath: string;
  /** chosen = from Settings, recent = your latest Claude Code session's folder */
  folderSource: "chosen" | "recent" | "none" | "squad";
  hasProject: boolean;
  hasConversation: boolean;
  /** Chatting with this squad pet (a copy of its session), or null. */
  squadName: string | null;
}

export interface SquadMember {
  id: string;
  name: string;
  project: string;
  phase: StatusView["phase"];
  activity: string;
  mood: Mood;
  level: number;
  stage: number;
  xpIntoLevel: number;
  xpForLevel: number;
  tasks: number;
  tools: number;
  minutes: number;
  character: string;
  species: string;
  color: string;
  hasChat: boolean;
}

export interface CharacterInfo {
  id: string;
  name: string;
  added: string;
}

export interface PetView {
  mood: Mood;
  status: StatusView | null;
  permission: PermView | null;
  toast: Toast | null;
  offer: Offer | null;
  chat: ChatView;
  chatOpen: boolean;
  dropHover: boolean;
  quickActions: ActionView[];
  progress: ProgressView | null;
  look: Look;
  emotes: ActionView[];
  quests: QuestView[];
  briefing: BriefingView | null;
  quiz: QuizView | null;
  squad: SquadMember[];
  /** Imported characters (id + name). */
  characters: ActionView[];
  /** Anime pets you've unlocked (id + name). */
  pets: ActionView[];
  /** Auto-allow is on (timed or full); null = off. */
  autoAllow: { full: boolean; minutesLeft: number | null; allowed: number } | null;
  /** Claude / Codex usage limits (null = turned off). */
  limits: LimitsView | null;
  /** The token detective's case report, while it's open. */
  caseReport: CaseReport | null;
  followMouse: boolean;
  hooksInstalled: boolean;
  gameActive: boolean;
}

export type ChatMode = "ask" | "readOnly" | "acceptEdits";

export interface Settings {
  pet: {
    monitor: string;
    character: string;
    position: number;
    statusLine: boolean;
    followMouse: boolean;
    showOnPermission: boolean;
    showOnDone: boolean;
    showOnAttention: boolean;
  };
  permissions: { enabled: boolean; timeoutSecs: number };
  chat: { enabled: boolean; projectDir: string; mode: ChatMode; keepConversation: boolean; claudePath: string };
  gameMode: boolean;
  quickActions: { enabled: boolean; actions: QuickAction[] };
  dropFiles: boolean;
  errorWatcher: boolean;
  health: boolean;
  progression: { enabled: boolean; neglect: boolean; hat: string; color: string; aura: string; pet: string };
  breaks: { enabled: boolean; intervalMins: number };
  sounds: {
    enabled: boolean;
    volume: number;
    taskDone: boolean;
    needsYou: boolean;
    problems: boolean;
    levelUp: boolean;
    breaks: boolean;
  };
  briefing: { enabled: boolean };
  learn: { enabled: boolean; everyMins: number };
  quests: { enabled: boolean; difficulty: "easy" | "normal" | "hard"; perDay: number; kinds: string[] };
  github: { enabled: boolean; everyMins: number };
  squad: { enabled: boolean; maxShown: number };
  autoAllow: { full: boolean; never: string[]; outsideProject: boolean };
  limits: { enabled: boolean; warn: boolean; warnPercent: number };
  detective: { enabled: boolean; weekly: boolean; cacheReminder: boolean };
}

export interface LimitWindow {
  label: string;
  used: number | null;
  usedText: string;
  resetsText: string;
  forecast: string;
  asOf: string;
  stale: boolean;
  exact: boolean;
  tight: boolean;
}

export interface CaseReport {
  running: boolean;
  period: string;
  summary: string;
  findings: { title: string; detail: string; tip: string; share: string; canFix: boolean }[];
  footnote: string;
}

export interface LimitsView {
  agents: { agent: string; plan: string; windows: LimitWindow[]; emptyHint: string }[];
}

export interface AutoAllowEntry {
  at: string;
  agent?: string;
  project: string;
  what: string;
  mode: "timed" | "full";
}

export interface QuickAction {
  id: string;
  label: string;
  prompt: string;
  readOnly: boolean;
}

export interface MonitorInfo {
  name: string;
  label: string;
  primary: boolean;
}

export interface HooksStatus {
  state: "installed" | "outdated" | "notInstalled" | "error";
  detail: string | null;
  settingsPath: string;
  hookPath: string;
  backupsDir: string;
}

export interface DiffLine {
  tag: "+" | "-" | " " | "gap";
  text: string;
}

export interface HooksPreview {
  install: boolean;
  changed: boolean;
  diff: DiffLine[];
  token: string;
  settingsPath: string;
  reformatted: boolean;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  claudePath: string | null;
  fullscreenNow: boolean;
  pipeError: string | null;
}
