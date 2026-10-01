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
  kind: "done" | "attention" | "failed" | "info";
  text: string;
  project: string;
}

export interface ChatView {
  enabled: boolean;
  busy: boolean;
  reply: string;
  activity: string;
  error: string | null;
  project: string;
  hasProject: boolean;
  hasConversation: boolean;
}

export interface PetView {
  mood: Mood;
  status: StatusView | null;
  permission: PermView | null;
  toast: Toast | null;
  chat: ChatView;
  chatOpen: boolean;
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
