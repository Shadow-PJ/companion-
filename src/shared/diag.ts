// Sends a diagnostic line to Glowby's local log (glowby.log). Used to debug
// focus and typing. It records events only, never the text you type.

import { invoke } from "@tauri-apps/api/core";

export function diag(message: string) {
  void invoke("js_log", { message }).catch(() => {});
}

window.addEventListener("error", (e) => diag(`error: ${e.message} @ ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => diag(`unhandled rejection: ${String(e.reason)}`));
window.addEventListener("focus", () => diag("window focus event (page can receive keys)"));
window.addEventListener("blur", () => diag("window blur event"));
