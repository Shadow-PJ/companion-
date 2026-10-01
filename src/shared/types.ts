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
}

export interface CosmeticView {
  id: string;
  kind: "hat" | "color" | "emote";
  name: string;
  unlocked: boolean;
  requirement: string;
}

export interface ProgressInfo {
  view: ProgressView;
  look: Look;
  stats: { tasks: number; fixes: number; testsPassed: number; commits: number; breaks: number };
  bestStreak: number;
  stages: [number, string][];
  cosmetics: CosmeticView[];
}

export interface Offer {
  kind: "clipboardError" | "failingChecks" | "break";
  title: string;
  detail: string;
  project: string;
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
  folderSource: "chosen" | "recent" | "none";
  hasProject: boolean;
  hasConversation: boolean;
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
  followMouse: boolean;
  hooksInstalled: boolean;
  gameActive: boolean;
}

export type ChatMode = "ask" | "readOnly" | "acceptEdits";

export interface Settings {
  pet: {
    monitor: string;
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
  progression: { enabled: boolean; neglect: boolean; hat: string; color: string };
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
