// Dev-only gallery (gallery.html, served by `npm run vite:dev`): draws every
// stage, mood, hat, colour and emote side by side, to check the art quickly.

import type { Mood } from "../pet/character/palette";
import { type Appearance, PetRenderer } from "../pet/renderer";

const root = document.getElementById("gallery")!;
if (location.search.includes("light")) document.body.classList.add("light");

const base: Appearance = { stage: 0, hat: "", color: "periwinkle", weak: false };

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
    if (emote) {
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
