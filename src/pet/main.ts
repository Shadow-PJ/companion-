// Entry point of the pet window: wires events from Rust to the renderer and bubble.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { PetView } from "../shared/types";
import { Bubble } from "./bubble";
import { BODY_X, BODY_Y, PetRenderer } from "./renderer";

const stage = document.getElementById("stage")!;
const canvas = document.getElementById("pet") as HTMLCanvasElement;
const bubbleEl = document.getElementById("bubble")!;

const renderer = new PetRenderer(canvas);
const bubble = new Bubble(bubbleEl, () => scheduleRegions());
let visible = false;
let followMouse = true;

function apply(view: PetView) {
  followMouse = view.followMouse;
  if (!followMouse) renderer.lookToward(0, 0);
  renderer.setMood(view.mood);
  bubble.render(view);
}

// ---- show / hide (slide animation is pure CSS) ----
function setVisible(show: boolean) {
  visible = show;
  if (show) {
    renderer.start();
    requestAnimationFrame(() => stage.classList.add("shown"));
  } else {
    stage.classList.remove("shown");
    window.setTimeout(() => {
      if (!visible) renderer.stop(); // fully paused while hidden
    }, 300);
  }
}

// ---- eyes follow the mouse (cursor position comes from Rust, ~30x/s while visible) ----
function onCursor(p: { x: number; y: number }) {
  if (!followMouse) return;
  const r = canvas.getBoundingClientRect();
  const dx = p.x - (r.left + BODY_X);
  const dy = p.y - (r.top + BODY_Y);
  const dist = Math.hypot(dx, dy) || 1;
  const strength = Math.min(dist / 140, 1);
  renderer.lookToward((dx / dist) * strength, (dy / dist) * strength);
}

// ---- click-through: tell Rust which rectangles are "solid" ----
let regionsQueued = false;
function scheduleRegions() {
  if (regionsQueued) return;
  regionsQueued = true;
  window.setTimeout(() => {
    regionsQueued = false;
    const rects: { x: number; y: number; w: number; h: number }[] = [];
    const c = canvas.getBoundingClientRect();
    rects.push({ x: c.left + 30, y: Math.max(c.top, 0), w: c.width - 60, h: c.height - 18 });
    if (!bubbleEl.hidden) {
      const b = bubbleEl.getBoundingClientRect();
      rects.push({ x: b.left - 4, y: b.top - 8, w: b.width + 8, h: b.height + 12 });
    }
    void invoke("set_hit_regions", { regions: rects });
  }, 0);
}
new ResizeObserver(scheduleRegions).observe(bubbleEl);
stage.addEventListener("transitionend", scheduleRegions);

// ---- click = chat, drag = move along the top edge ----
let press: { x: number; y: number; dragging: boolean } | null = null;
canvas.addEventListener("pointerdown", (e) => {
  if (e.button === 0) press = { x: e.screenX, y: e.screenY, dragging: false };
});
window.addEventListener("pointermove", (e) => {
  if (!press || press.dragging) return;
  if (Math.hypot(e.screenX - press.x, e.screenY - press.y) > 6) {
    press.dragging = true;
    void invoke("drag_start");
  }
});
window.addEventListener("pointerup", (e) => {
  const wasClick = press && !press.dragging && e.target === canvas;
  press = null;
  if (wasClick) bubble.toggleChat();
});
// No browser context menu. (Right-click will open quick actions in Phase 2.)
window.addEventListener("contextmenu", (e) => e.preventDefault());

// ---- events from Rust ----
void listen<PetView>("pet://view", (e) => apply(e.payload));
void listen<boolean>("pet://visibility", (e) => setVisible(e.payload));
void listen<{ x: number; y: number }>("pet://cursor", (e) => onCursor(e.payload));
void listen("pet://blur", () => bubble.onBlur());

void invoke<PetView>("pet_ready").then(apply);
