// Entry point of the pet window: wires events from Rust to the renderer and bubble.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { diag } from "../shared/diag";
import type { PetView } from "../shared/types";
import { Bubble } from "./bubble";
import { strokeDetector } from "./petting";
import { keepOnly } from "./character/images";
import { BODY_X, PetRenderer } from "./renderer";
import { playSound } from "./sound";
import { Squad } from "./squad";

const stage = document.getElementById("stage")!;
const canvas = document.getElementById("pet") as HTMLCanvasElement;
const squadCanvas = document.getElementById("squad") as HTMLCanvasElement;
const bubbleEl = document.getElementById("bubble")!;

const renderer = new PetRenderer(canvas);
const squad = new Squad(squadCanvas);
renderer.onFrame = (t, dt) => squad.draw(t, dt);
const bubble = new Bubble(bubbleEl, () => scheduleRegions());
let visible = false;
let followMouse = true;

function apply(view: PetView) {
  followMouse = view.followMouse;
  if (!followMouse) renderer.lookToward(0, 0);
  renderer.setMood(view.mood);
  renderer.setAppearance(view.look);
  squad.set(view.squad);
  squad.selected = bubble.squadSelected;
  bubbleEl.style.marginTop = `-${renderer.bubbleOverlap()}px`;
  // Only keep the pictures someone is wearing in memory.
  keepOnly([view.look.character, ...view.squad.map((m) => m.character)].filter(Boolean));
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
    bubble.onHidden();
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
  const dy = p.y - (r.top + renderer.bodyY());
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
    if (!squadCanvas.hidden) {
      const s = squadCanvas.getBoundingClientRect();
      for (const slot of squad.slots()) rects.push({ x: s.left + slot.x, y: s.top + slot.y, w: slot.w, h: slot.h });
    }
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
  diag(`pointer down on Glowby (button ${e.button})`);
  if (e.button === 0) press = { x: e.screenX, y: e.screenY, dragging: false };
});
window.addEventListener("pointermove", (e) => {
  if (!press || press.dragging) return;
  if (Math.hypot(e.screenX - press.x, e.screenY - press.y) > 10) {
    press.dragging = true;
    void invoke("drag_start");
  }
});
window.addEventListener("pointerup", (e) => {
  const wasClick = press && !press.dragging && e.target === canvas;
  if (press) diag(`pointer up: click=${!!wasClick} dragged=${press.dragging}`);
  press = null;
  if (wasClick) bubble.toggleChat();
});
// Click a squad pet = its card (what it's doing, its look, chat).
squadCanvas.addEventListener("click", (e) => {
  const r = squadCanvas.getBoundingClientRect();
  const id = squad.hit(e.clientX - r.left, e.clientY - r.top);
  if (id) bubble.toggleSquad(id);
  squad.selected = bubble.squadSelected;
});
// ---- petting: stroke back and forth over a pet (no button pressed) ----
let lastStroke = 0;
canvas.addEventListener(
  "pointermove",
  strokeDetector(() => {
    renderer.pet();
    const now = performance.now();
    // one "petting" per session (a pause of 3 s starts a new one); Rust limits the XP
    if (now - lastStroke > 3000) void invoke("pet_petted");
    lastStroke = now;
  }),
);
squadCanvas.addEventListener(
  "pointermove",
  strokeDetector((x, y) => {
    const r = squadCanvas.getBoundingClientRect();
    const id = squad.hit(x - r.left, y - r.top);
    if (id) squad.pet(id);
  }),
);

// Right-click Glowby = quick actions. No browser context menu anywhere.
window.addEventListener("contextmenu", (e) => e.preventDefault());
canvas.addEventListener("contextmenu", () => bubble.toggleMenu());

// ---- events from Rust ----
void listen<PetView>("pet://view", (e) => apply(e.payload));
void listen<boolean>("pet://visibility", (e) => setVisible(e.payload));
void listen<{ x: number; y: number }>("pet://cursor", (e) => onCursor(e.payload));
void listen("pet://blur", () => bubble.onBlur());
// Sounds arrive even while Glowby is hidden (Rust already checked Settings and game mode).
void listen<{ name: string; volume: number }>("pet://sound", (e) => playSound(e.payload.name, e.payload.volume));
void listen<string>("pet://emote", (e) => renderer.playEmote(e.payload));
void listen<{ amount: number; reason: string }>("pet://xp", (e) => {
  if (visible) renderer.addFloater(`+${e.payload.amount} XP`);
});
void listen<{ id: string; text: string }>("pet://squad-levelup", (e) => {
  if (visible) squad.levelUp(e.payload.id, e.payload.text);
});
bubble.onEmote = (id) => void invoke("play_emote", { id });
bubble.onSquadChange = () => {
  squad.selected = bubble.squadSelected;
};

void invoke<PetView>("pet_ready").then(apply);
