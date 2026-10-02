// Dev-only gallery (gallery.html, served by `npm run vite:dev`): draws every
// stage, mood, hat, colour and emote side by side, to check the art quickly.

import type { Mood } from "../pet/character/palette";
import { setCharacterImage } from "../pet/character/images";
import { type Appearance, DEFAULT_APPEARANCE, PetRenderer } from "../pet/renderer";

const root = document.getElementById("gallery")!;
if (location.search.includes("light")) document.body.classList.add("light");

const base: Appearance = { ...DEFAULT_APPEARANCE };

// A stand-in "imported character" drawn in code (the gallery has no picture files).
async function testCharacter() {
  const c = new OffscreenCanvas(256, 256);
  const g = c.getContext("2d")!;
  const sky = g.createLinearGradient(0, 0, 256, 256);
  sky.addColorStop(0, "#2B3A67");
  sky.addColorStop(1, "#E07A5F");
  g.fillStyle = sky;
  g.fillRect(0, 0, 256, 256);
  g.fillStyle = "#F2CC8F";
  g.beginPath();
  g.arc(128, 140, 70, 0, Math.PI * 2);
  g.fill();
  g.fillStyle = "#3D405B";
  for (const x of [102, 154]) {
    g.beginPath();
    g.arc(x, 132, 9, 0, Math.PI * 2);
    g.fill();
  }
  g.fillRect(70, 52, 116, 30);
  setCharacterImage("test", c.transferToImageBitmap());
}
await testCharacter();

function row(title: string, items: [string, Mood, Partial<Appearance>, string?][]) {
  const h = document.createElement("h2");
  h.textContent = title;
  const r = document.createElement("div");
  r.className = "row";
  for (const [caption, mood, look, emote] of items) {
    const fig = document.createElement("figure");
    const canvas = document.createElement("canvas");
    const cap = document.createElement("figcaption");
    cap.textContent = caption;
    fig.append(canvas, cap);
    r.append(fig);
    const renderer = new PetRenderer(canvas);
    renderer.drawStill(mood, { ...base, ...look });
    if (emote === "live") {
      renderer.start();
    } else if (emote) {
      renderer.start();
      renderer.playEmote(emote);
      setInterval(() => renderer.playEmote(emote), 2200);
    }
  }
  root.append(h, r);
}

row("Stages", [
  ["Little", "idle", { stage: 0 }],
  ["Lantern", "idle", { stage: 1 }],
  ["Starlit", "idle", { stage: 2 }],
  ["Aurora", "idle", { stage: 3 }],
  ["Weak (ignored)", "sleepy", { stage: 1, weak: true }],
]);
row(
  "Moods (Lantern)",
  (["idle", "working", "happy", "alert", "sleepy", "sick"] as Mood[]).map((m) => [m, m, { stage: 1 }]),
);
row(
  "Hats",
  ["sprout", "party", "beanie", "headphones", "crown", "wizard", "gradcap"].map((h) => [h, "happy", { hat: h }]),
);
row(
  "Colours (Starlit)",
  ["periwinkle", "mint", "peach", "lilac", "rose", "aqua"].map((c) => [c, "idle", { stage: 2, color: c }]),
);
row(
  "Emotes (animated)",
  ["wave", "heart", "spin", "dance", "fireworks"].map((e) => [e, "happy", { stage: 1 }, e]),
);
row(
  "Auras (animated)",
  ["sparkle", "flame", "sakura", "lightning", "cursed", "infinity", "sun", "rainbow"].map((a) => [a, "idle", { stage: 1, aura: a }, "live"]),
);
row(
  "Imported character (moods)",
  (["idle", "working", "happy", "alert", "sleepy", "sick"] as Mood[]).map((m) => [m, m, { character: "test", stage: 1 }, "live"]),
);
row(
  "Imported character (stages, hat, auras)",
  [
    ["Little", "idle", { character: "test", stage: 0 }],
    ["Starlit + crown", "idle", { character: "test", stage: 2, hat: "crown" }, "live"],
    ["Aurora + flame", "happy", { character: "test", stage: 3, aura: "flame" }, "live"],
    ["Golden power-up", "working", { character: "test", stage: 1, aura: "lightning" }, "live"],
    ["Cursed energy", "idle", { character: "test", stage: 1, aura: "cursed", hat: "wizard" }, "live"],
  ],
);
for (const sp of ["neko", "kitsune", "slime", "ninja", "mecha"]) {
  row(
    `Pet: ${sp} (moods)`,
    (["idle", "working", "happy", "alert", "sleepy", "sick"] as Mood[]).map((m) => [m, m, { species: sp, stage: 1 }]),
  );
}
row(
  "Pet emotes (animated, Neko)",
  ["wave", "cheer", "jump", "laugh", "peace", "shy", "sparkle", "heart", "dance"].map((e) => [e, "idle", { species: "neko" }, e]),
);
row(
  "Pets: stages, hats, auras",
  [
    ["Kitsune L0", "idle", { species: "kitsune", stage: 0 }],
    ["Kitsune + flame", "happy", { species: "kitsune", stage: 2, aura: "flame" }, "live"],
    ["Ninja + crown", "idle", { species: "ninja", stage: 1, hat: "crown" }, "live"],
    ["Robot + infinity", "working", { species: "mecha", stage: 3, aura: "infinity" }, "live"],
    ["Slime (mint)", "idle", { species: "slime", color: "mint", stage: 1 }, "live"],
  ],
);
