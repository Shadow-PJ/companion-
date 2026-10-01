// The animation loop. It runs ONLY while Glowby is visible and stops completely
// when hidden, so a hidden Glowby costs no CPU for drawing.

import { drawGlowby } from "./character/glowby";
import { blendStyle, type Mood, type MoodStyle, STYLES, styleFor } from "./character/palette";

/** Canvas size in CSS pixels. */
export const CANVAS_W = 168;
export const CANVAS_H = 172;
export const BODY_X = CANVAS_W / 2;
const BASE_BODY_Y = 54;
/** Tall hats need Glowby to sit a little lower so the hat stays on screen. */
const HAT_LIFT: Record<string, number> = { sprout: 10, party: 18, wizard: 24, gradcap: 10, crown: 9, beanie: 7, headphones: 3 };
/** Never draw more often than 60 times a second, even on 144 Hz screens. */
const MIN_FRAME_MS = 1000 / 60 - 1;
const EMOTE_SECONDS = 1.8;
const FLOATER_SECONDS = 1.5;

export interface Appearance {
  stage: number;
  hat: string;
  color: string;
  weak: boolean;
}

export class PetRenderer {
  private ctx: CanvasRenderingContext2D;
  private running = false;
  private raf = 0;
  private lastFrame = 0;
  private lastDraw = 0;
  private mood: Mood = "idle";
  private moodSince = 0;
  private style: MoodStyle = { ...STYLES.idle };
  private look = { x: 0, y: 0 };
  private lookTarget = { x: 0, y: 0 };
  private nextBlink = 2;
  private blinkStart = -1;
  private appearance: Appearance = { stage: 0, hat: "", color: "periwinkle", weak: false };
  private emote: { id: string; start: number } | null = null;
  private floaters: { text: string; start: number }[] = [];

  constructor(private canvas: HTMLCanvasElement) {
    this.ctx = canvas.getContext("2d")!;
    this.resize();
    matchMedia(`(resolution: ${devicePixelRatio}dppx)`).addEventListener("change", () => this.resize());
  }

  /** Sharp drawing on high-DPI screens: more device pixels, same CSS size. */
  private resize() {
    const dpr = Math.min(window.devicePixelRatio || 1, 3);
    this.canvas.width = Math.round(CANVAS_W * dpr);
    this.canvas.height = Math.round(CANVAS_H * dpr);
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  /** Where the bell's centre is inside the canvas (eyes aim from here). */
  bodyY() {
    return BASE_BODY_Y + (HAT_LIFT[this.appearance.hat] ?? 0);
  }

  setMood(mood: Mood) {
    if (mood !== this.mood) {
      this.mood = mood;
      this.moodSince = performance.now() / 1000;
    }
  }

  setAppearance(a: Appearance) {
    this.appearance = a;
  }

  playEmote(id: string) {
    this.emote = { id, start: performance.now() / 1000 };
  }

  /** "+10 XP" floating up next to Glowby. */
  addFloater(text: string) {
    this.floaters.push({ text, start: performance.now() / 1000 });
    if (this.floaters.length > 4) this.floaters.shift();
  }

  /** Direction the eyes should look, each axis -1..1. */
  lookToward(x: number, y: number) {
    this.lookTarget = { x: Math.max(-1, Math.min(1, x)), y: Math.max(-1, Math.min(1, y)) };
  }

  start() {
    if (this.running) return;
    this.running = true;
    this.lastFrame = performance.now();
    this.raf = requestAnimationFrame(this.frame);
  }

  stop() {
    this.running = false;
    cancelAnimationFrame(this.raf);
    this.emote = null;
    this.floaters = [];
  }

  /** Draws a single still frame (Settings previews). */
  drawStill(mood: Mood, appearance?: Appearance) {
    if (appearance) this.appearance = appearance;
    this.setMood(mood);
    this.style = styleFor(mood, this.appearance.color);
    this.draw(1.2, 0);
  }

  private frame = (now: number) => {
    if (!this.running) return;
    this.raf = requestAnimationFrame(this.frame);
    if (now - this.lastDraw < MIN_FRAME_MS) return;
    const dt = Math.min((now - this.lastFrame) / 1000, 0.1);
    this.lastFrame = now;
    this.lastDraw = now;
    this.draw(now / 1000, dt);
  };

  private draw(t: number, dt: number) {
    // Ease colours and eye position instead of jumping (frame-rate independent).
    this.style = blendStyle(this.style, styleFor(this.mood, this.appearance.color), dt ? 1 - Math.exp(-dt * 5) : 1);
    const k = 1 - Math.exp(-dt * 12);
    this.look.x += (this.lookTarget.x - this.look.x) * k;
    this.look.y += (this.lookTarget.y - this.look.y) * k;

    let emote: { id: string; p: number } | null = null;
    if (this.emote) {
      const p = (t - this.emote.start) / EMOTE_SECONDS;
      if (p >= 1 || p < 0) this.emote = null;
      else emote = { id: this.emote.id, p };
    }

    const ctx = this.ctx;
    ctx.clearRect(0, 0, CANVAS_W, CANVAS_H);
    drawGlowby(ctx, {
      t,
      moodAge: t - this.moodSince,
      mood: this.mood,
      style: this.style,
      look: this.look,
      open: this.blink(t),
      x: BODY_X,
      y: this.bodyY(),
      appearance: this.appearance,
      emote,
    });
    this.drawFloaters(t);
  }

  private drawFloaters(t: number) {
    const ctx = this.ctx;
    this.floaters = this.floaters.filter((f) => t - f.start < FLOATER_SECONDS && t >= f.start);
    ctx.save();
    ctx.textAlign = "center";
    ctx.font = "700 12px 'Segoe UI', sans-serif";
    this.floaters.forEach((f, i) => {
      const p = (t - f.start) / FLOATER_SECONDS;
      ctx.globalAlpha = p < 0.7 ? 1 : 1 - (p - 0.7) / 0.3;
      const fx = BODY_X + 50 - i * 4;
      const fy = this.bodyY() - 6 - p * 26;
      ctx.lineWidth = 3;
      ctx.strokeStyle = "rgba(20,24,60,0.55)";
      ctx.strokeText(f.text, fx, fy);
      ctx.fillStyle = "#FAC775";
      ctx.fillText(f.text, fx, fy);
    });
    ctx.restore();
  }

  /** Natural blinking: every 2.5–5.5 s, sometimes twice in a row. */
  private blink(t: number): number {
    if (this.blinkStart < 0 && t >= this.nextBlink) {
      this.blinkStart = t;
    }
    if (this.blinkStart >= 0) {
      const p = (t - this.blinkStart) / 0.14;
      if (p >= 1) {
        this.blinkStart = -1;
        this.nextBlink = t + (Math.random() < 0.15 ? 0.18 : 2.5 + Math.random() * 3);
        return 1;
      }
      return Math.abs(1 - p * 2);
    }
    return 1;
  }
}
