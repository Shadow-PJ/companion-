// The animation loop. It runs ONLY while Glowby is visible and stops completely
// when hidden, so a hidden Glowby costs no CPU for drawing.

import { drawAura } from "./character/auras";
import { AVATAR_R, drawAvatar } from "./character/avatar";
import { drawChibi, SPECIES } from "./character/chibi";
import { drawGlowby } from "./character/glowby";
import { characterImage, onImageLoaded } from "./character/images";
import { blendStyle, type Mood, type MoodStyle, STYLES, styleFor } from "./character/palette";

/** Canvas size in CSS pixels. */
export const CANVAS_W = 168;
export const CANVAS_H = 172;
export const BODY_X = CANVAS_W / 2;
const BASE_BODY_Y = 54;
/** The round character icon sits a little lower than the jellyfish's bell. */
const AVATAR_BODY_Y = 60;
/** Anime pets: (x, y) is the centre of the head; ears reach ~47 px above it. */
const CHIBI_BODY_Y = 54;
/** Tall hats need Glowby to sit a little lower so the hat stays on screen. */
const HAT_LIFT: Record<string, number> = { sprout: 10, party: 18, wizard: 24, gradcap: 10, crown: 9, beanie: 7, headphones: 3, detective: 10 };
/** Never draw more often than 60 times a second, even on 144 Hz screens. */
const ACTIVE_FRAME_MS = 1000 / 60 - 1;
const IDLE_FRAME_MS = 1000 / 24 - 1;
const EMOTE_SECONDS = 1.8;
const FLOATER_SECONDS = 1.5;

export interface Appearance {
  stage: number;
  hat: string;
  color: string;
  weak: boolean;
  aura: string;
  /** Imported character id, "" = not used. Wins over `species`. */
  character: string;
  /** Anime pet ("neko", "kitsune" …), "" = Glowby the jellyfish. */
  species: string;
}

export const DEFAULT_APPEARANCE: Appearance = { stage: 0, hat: "", color: "periwinkle", weak: false, aura: "", character: "", species: "" };

/** Little things a pet does on its own while you watch (ids are emote ids). */
const IDLE_ACTIONS = ["idle-look", "idle-stretch", "idle-hop", "idle-wag", "wave"];
const PET_SECONDS = 0.9;

export class PetRenderer {
  /** Extra drawing in the same frame (the squad pets), so there is only one loop. */
  onFrame: ((t: number, dt: number) => void) | null = null;
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
  private appearance: Appearance = { ...DEFAULT_APPEARANCE };
  private emote: { id: string; start: number } | null = null;
  private floaters: { text: string; start: number }[] = [];
  private still: Mood | null = null;
  /** Petting lasts this long after your last stroke. */
  private petUntil = 0;
  private hearts: { x: number; start: number }[] = [];
  private nextHeart = 0;
  private nextIdle = 0;
  private lastGreeting = -1e9;
  private reducedMotion = false;
  private frameTimer = 0;
  private greetingTimer = 0;
  private unsubscribeImage: () => void;
  private resolution = matchMedia(`(resolution: ${devicePixelRatio}dppx)`);
  private resolutionChanged = () => this.resize();

  constructor(private canvas: HTMLCanvasElement) {
    this.ctx = canvas.getContext("2d")!;
    this.resize();
    this.resolution.addEventListener("change", this.resolutionChanged);
    // A still preview drawn before its picture loaded: draw it again once it's there.
    this.unsubscribeImage = onImageLoaded(() => {
      if (this.still && !this.running) this.drawStill(this.still);
    });
  }

  setReducedMotion(value: boolean) { this.reducedMotion = value || matchMedia("(prefers-reduced-motion: reduce)").matches; }

  dispose() {
    this.stop();
    this.unsubscribeImage();
    this.resolution.removeEventListener("change", this.resolutionChanged);
    this.onFrame = null;
    this.canvas.width = this.canvas.height = 1;
  }

  /** Sharp drawing on high-DPI screens: more device pixels, same CSS size. */
  private resize() {
    const dpr = Math.min(window.devicePixelRatio || 1, 3);
    this.canvas.width = Math.round(CANVAS_W * dpr);
    this.canvas.height = Math.round(CANVAS_H * dpr);
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  private isChibi() {
    return !this.appearance.character && !!SPECIES[this.appearance.species];
  }

  /** Where the body's centre is inside the canvas (eyes aim from here). */
  bodyY() {
    const base = this.appearance.character ? AVATAR_BODY_Y : this.isChibi() ? CHIBI_BODY_Y : BASE_BODY_Y;
    return base + (HAT_LIFT[this.appearance.hat] ?? 0);
  }

  /** How far the speech bubble may tuck up under the body (no tentacles hanging down). */
  bubbleOverlap() {
    if (this.appearance.character) return Math.max(14, CANVAS_H - (this.bodyY() + AVATAR_R + 18));
    if (this.isChibi()) return Math.max(14, CANVAS_H - (this.bodyY() + 62 + 8));
    return 14;
  }

  /** You're stroking him with the mouse: happy face, hearts, a little squish. */
  pet() {
    const now = performance.now() / 1000;
    this.petUntil = now + PET_SECONDS;
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
    this.still = null;
    this.lastFrame = performance.now();
    this.lastDraw = 0;
    this.raf = requestAnimationFrame(this.frame);
    // Say hi when sliding out (not every time you hover by).
    const now = performance.now() / 1000;
    if (now - this.lastGreeting > 90) {
      this.lastGreeting = now;
      this.greetingTimer = window.setTimeout(() => {
        if (this.running && !this.emote && this.mood !== "alert" && this.mood !== "sick") this.playEmote("wave");
      }, 450);
    }
    this.nextIdle = now + 8 + Math.random() * 8;
  }

  stop() {
    this.running = false;
    cancelAnimationFrame(this.raf);
    window.clearTimeout(this.frameTimer);
    window.clearTimeout(this.greetingTimer);
    this.emote = null;
    this.floaters = [];
    this.hearts = [];
    this.petUntil = 0;
  }

  /** Now and then, while idle, do something small on your own. */
  private idleLife(t: number) {
    if (!this.running || this.reducedMotion || this.emote || t < this.nextIdle) return;
    this.nextIdle = t + 10 + Math.random() * 14;
    if (this.mood !== "idle" && this.mood !== "happy") return;
    if (t < this.petUntil) return;
    const pick = IDLE_ACTIONS[Math.floor(Math.random() * IDLE_ACTIONS.length)];
    this.emote = { id: pick, start: t };
  }

  /** Draws a single still frame (Settings previews). */
  drawStill(mood: Mood, appearance?: Appearance) {
    if (appearance) this.appearance = appearance;
    this.still = mood;
    this.setMood(mood);
    this.style = styleFor(mood, this.appearance.color);
    this.draw(1.2, 0);
  }

  private frame = (now: number) => {
    if (!this.running) return;
    const lively = !!this.emote || now / 1000 < this.petUntil || this.floaters.length > 0;
    const interval = this.reducedMotion ? (lively ? 1000 / 30 : 1000 / 6) : lively ? ACTIVE_FRAME_MS : IDLE_FRAME_MS;
    const remaining = interval - (now - this.lastDraw);
    if (remaining > 1) {
      this.frameTimer = window.setTimeout(() => { if (this.running) this.raf = requestAnimationFrame(this.frame); }, remaining);
      return;
    }
    this.frameTimer = window.setTimeout(() => { if (this.running) this.raf = requestAnimationFrame(this.frame); }, Math.max(0, interval - 8));
    const dt = Math.min((now - this.lastFrame) / 1000, 0.1);
    this.lastFrame = now;
    this.lastDraw = now;
    this.draw(now / 1000, dt);
    this.onFrame?.(now / 1000, dt);
  };

  private draw(t: number, dt: number) {
    // Ease colours and eye position instead of jumping (frame-rate independent).
    this.style = blendStyle(this.style, styleFor(this.mood, this.appearance.color), dt ? 1 - Math.exp(-dt * 5) : 1);
    const k = 1 - Math.exp(-dt * 12);
    this.look.x += (this.lookTarget.x - this.look.x) * k;
    this.look.y += (this.lookTarget.y - this.look.y) * k;

    if (dt) this.idleLife(t);
    let emote: { id: string; p: number } | null = null;
    if (this.emote) {
      const p = (t - this.emote.start) / EMOTE_SECONDS;
      if (p >= 1 || p < 0) this.emote = null;
      else emote = { id: this.emote.id, p };
    }
    // "Looking around" moves the eyes left and right for a moment.
    if (emote?.id === "idle-look") {
      this.look.x = Math.sin(emote.p * Math.PI * 2) * 0.9;
      this.look.y = -0.2;
    }
    const petting = Math.max(0, Math.min(1, (this.petUntil - t) / PET_SECONDS));

    const ctx = this.ctx;
    const a = this.appearance;
    const y = this.bodyY();
    ctx.clearRect(0, 0, CANVAS_W, CANVAS_H);
    const frame = {
      t: this.reducedMotion ? 1.2 : t,
      moodAge: t - this.moodSince,
      mood: this.mood,
      style: this.style,
      look: this.look,
      open: this.blink(t),
      x: BODY_X,
      y,
      appearance: a,
      emote,
      petting,
    };
    // Auras are centred on the body: the icon, the pet, or the jellyfish's bell.
    const chibi = this.isChibi();
    const auraY = a.character ? y : chibi ? y + 16 : y + 6;
    const auraR = a.character ? AVATAR_R + 3 : chibi ? 40 : 38;
    const power = (this.mood === "sleepy" ? 0.45 : 1) * (a.weak ? 0.6 : 1);
    const motionTime = this.reducedMotion ? 1.2 : t;
    drawAura(ctx, a.aura, BODY_X, auraY, auraR, motionTime, "back", power);
    if (a.character) drawAvatar(ctx, frame, characterImage(a.character));
    else if (chibi) drawChibi(ctx, frame, a.species, a.color);
    else drawGlowby(ctx, frame);
    drawAura(ctx, a.aura, BODY_X, auraY, auraR, motionTime, "front", power);
    this.drawHearts(t, petting, y);
    this.drawFloaters(t);
  }

  /** Little hearts floating up while you pet him. */
  private drawHearts(t: number, petting: number, y: number) {
    if (petting > 0.3 && t >= this.nextHeart) {
      this.nextHeart = t + 0.32;
      this.hearts.push({ x: BODY_X + (Math.random() - 0.5) * 50, start: t });
      if (this.hearts.length > 8) this.hearts.shift();
    }
    this.hearts = this.hearts.filter((h) => t - h.start < 1.2);
    const ctx = this.ctx;
    for (const h of this.hearts) {
      const p = (t - h.start) / 1.2;
      ctx.save();
      ctx.globalAlpha = 1 - p;
      ctx.fillStyle = "#F06E96";
      const hx = h.x + Math.sin(p * 6 + h.x) * 4;
      const hy = y - 20 - p * 34;
      const s = 4.5 + p * 2;
      ctx.beginPath();
      ctx.moveTo(hx, hy + s * 0.35);
      ctx.bezierCurveTo(hx - s, hy - s * 0.4, hx - s * 0.4, hy - s, hx, hy - s * 0.45);
      ctx.bezierCurveTo(hx + s * 0.4, hy - s, hx + s, hy - s * 0.4, hx, hy + s * 0.35);
      ctx.fill();
      ctx.restore();
    }
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
