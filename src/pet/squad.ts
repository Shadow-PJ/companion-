// Squad mode: one small pet per running Claude Code session, sitting on both
// sides of Glowby. Drawn on one wide canvas behind Glowby's own, in the same
// animation frame (no second loop), and only while Glowby is visible.

import { AVATAR_R, drawAvatar } from "./character/avatar";
import { drawGlowby, type Frame } from "./character/glowby";
import { characterImage } from "./character/images";
import { blendStyle, type Mood, type MoodStyle, styleFor } from "./character/palette";
import type { SquadMember } from "../shared/types";

export const SQUAD_W = 480;
export const SQUAD_H = 104;
/** Pets are drawn at half size. */
const SCALE = 0.5;
const PET_Y = 38;
/** Slot centres, nearest to Glowby first, alternating right / left. */
const SLOTS = [348, 132, 398, 82, 448, 32];
const HALF_SLOT = 24;
const FLOATER_SECONDS = 1.6;

interface Live {
  style: MoodStyle;
  mood: Mood;
  moodSince: number;
  emote: { id: string; start: number } | null;
  floater: { text: string; start: number } | null;
}

export class Squad {
  private ctx: CanvasRenderingContext2D;
  private members: SquadMember[] = [];
  private live = new Map<string, Live>();
  selected: string | null = null;

  constructor(private canvas: HTMLCanvasElement) {
    this.ctx = canvas.getContext("2d")!;
    const dpr = Math.min(window.devicePixelRatio || 1, 3);
    canvas.width = Math.round(SQUAD_W * dpr);
    canvas.height = Math.round(SQUAD_H * dpr);
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  set(members: SquadMember[]) {
    this.members = members.slice(0, SLOTS.length);
    const ids = new Set(this.members.map((m) => m.id));
    for (const id of this.live.keys()) if (!ids.has(id)) this.live.delete(id);
    if (this.selected && !ids.has(this.selected)) this.selected = null;
    this.canvas.hidden = this.members.length === 0;
  }

  /** Clickable rectangles (CSS pixels inside the canvas) and who they belong to. */
  slots(): { id: string; x: number; y: number; w: number; h: number }[] {
    return this.members.map((m, i) => ({ id: m.id, x: SLOTS[i] - HALF_SLOT, y: 0, w: HALF_SLOT * 2, h: SQUAD_H }));
  }

  hit(x: number, y: number): string | null {
    return this.slots().find((s) => x >= s.x && x <= s.x + s.w && y >= s.y && y <= s.y + s.h)?.id ?? null;
  }

  /** "Lv 3!" over a pet that just levelled up, with a little spin. */
  levelUp(id: string, text: string) {
    const l = this.live.get(id);
    const now = performance.now() / 1000;
    if (l) {
      l.floater = { text, start: now };
      l.emote = { id: "spin", start: now };
    }
  }

  draw(t: number, dt: number) {
    if (this.members.length === 0) return;
    const ctx = this.ctx;
    ctx.clearRect(0, 0, SQUAD_W, SQUAD_H);
    this.members.forEach((m, i) => this.drawMember(ctx, m, SLOTS[i], t, dt));
  }

  private drawMember(ctx: CanvasRenderingContext2D, m: SquadMember, cx: number, t: number, dt: number) {
    let l = this.live.get(m.id);
    if (!l) {
      l = { style: styleFor(m.mood, m.color), mood: m.mood, moodSince: t, emote: null, floater: null };
      this.live.set(m.id, l);
    }
    if (l.mood !== m.mood) {
      l.mood = m.mood;
      l.moodSince = t;
    }
    l.style = blendStyle(l.style, styleFor(m.mood, m.color), dt ? 1 - Math.exp(-dt * 5) : 1);
    let emote: Frame["emote"] = null;
    if (l.emote) {
      const p = (t - l.emote.start) / 1.6;
      if (p >= 1 || p < 0) l.emote = null;
      else emote = { id: l.emote.id, p };
    }

    // Selected pet: a soft spotlight underneath.
    if (this.selected === m.id) {
      const g = ctx.createRadialGradient(cx, PET_Y + 6, 4, cx, PET_Y + 6, 30);
      g.addColorStop(0, "rgba(142,162,255,0.35)");
      g.addColorStop(1, "rgba(142,162,255,0)");
      ctx.fillStyle = g;
      ctx.fillRect(cx - 30, PET_Y - 24, 60, 60);
    }

    // Draw a full-size pet at (0, 0) and shrink it into its slot.
    ctx.save();
    ctx.translate(cx, PET_Y);
    ctx.scale(SCALE, SCALE);
    const frame: Frame = {
      t: t + cx * 0.01, // each pet bobs on its own beat
      moodAge: t - l.moodSince,
      mood: m.mood,
      style: l.style,
      look: { x: cx < SQUAD_W / 2 ? 0.5 : -0.5, y: 0.1 }, // they look toward Glowby
      open: 1,
      x: 0,
      y: 0,
      appearance: { stage: m.stage, hat: "", weak: false },
      emote,
    };
    if (m.character) drawAvatar(ctx, frame, characterImage(m.character));
    else drawGlowby(ctx, frame);
    ctx.restore();

    // Name and level under the pet.
    const labelY = m.character ? PET_Y + AVATAR_R * SCALE + 16 : PET_Y + 50;
    ctx.save();
    ctx.textAlign = "center";
    ctx.lineJoin = "round";
    ctx.font = "600 10.5px 'Segoe UI', sans-serif";
    const label = `${m.name} · ${m.level}`;
    ctx.lineWidth = 3;
    ctx.strokeStyle = "rgba(20,24,60,0.6)";
    ctx.strokeText(label, cx, labelY);
    ctx.fillStyle = "#F4F5FF";
    ctx.fillText(label, cx, labelY);
    ctx.restore();

    if (l.floater) {
      const p = (t - l.floater.start) / FLOATER_SECONDS;
      if (p >= 1) l.floater = null;
      else {
        ctx.save();
        ctx.textAlign = "center";
        ctx.font = "700 11px 'Segoe UI', sans-serif";
        ctx.globalAlpha = p < 0.7 ? 1 : 1 - (p - 0.7) / 0.3;
        const fy = PET_Y - 12 - p * 18;
        ctx.lineWidth = 3;
        ctx.strokeStyle = "rgba(20,24,60,0.55)";
        ctx.strokeText(l.floater.text, cx, fy);
        ctx.fillStyle = "#FAC775";
        ctx.fillText(l.floater.text, cx, fy);
        ctx.restore();
      }
    }
  }
}

