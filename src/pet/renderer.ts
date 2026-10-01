// The animation loop. It runs ONLY while Glowby is visible and stops completely
// when hidden, so a hidden Glowby costs no CPU for drawing.

import { drawGlowby } from "./character/glowby";
import { blendStyle, type Mood, type MoodStyle, STYLES } from "./character/palette";

/** Canvas size in CSS pixels. */
export const CANVAS_W = 168;
export const CANVAS_H = 156;
/** Where the bell's centre sits inside the canvas. */
export const BODY_X = CANVAS_W / 2;
export const BODY_Y = 54;
/** Never draw more often than 60 times a second, even on 144 Hz screens. */
const MIN_FRAME_MS = 1000 / 60 - 1;

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

  setMood(mood: Mood) {
    if (mood !== this.mood) {
      this.mood = mood;
      this.moodSince = performance.now() / 1000;
    }
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
  }

  /** Draws a single still frame (used by Settings for its logo). */
  drawStill(mood: Mood) {
    this.setMood(mood);
    this.style = { ...STYLES[mood] };
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
    this.style = blendStyle(this.style, STYLES[this.mood], 1 - Math.exp(-dt * 5));
    const k = 1 - Math.exp(-dt * 12);
    this.look.x += (this.lookTarget.x - this.look.x) * k;
    this.look.y += (this.lookTarget.y - this.look.y) * k;

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
      y: BODY_Y,
    });
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
