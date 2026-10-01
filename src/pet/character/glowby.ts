// Glowby, drawn entirely in code with the Canvas 2D API.
// A jellyfish: a pulsing bell (dome), a soft glow, waving tentacles, a face.
//
// Four evolution stages build on each other:
//   0 Little Glowby   – the original
//   1 Lantern Glowby  – warm light inside the bell, lights along the rim
//   2 Starlit Glowby  – glowing crown spots, longer tentacles, drifting sparkles
//   3 Aurora Glowby   – colour-shifting aurora bell, a trail of light

import { drawFace } from "./face";
import { drawHat } from "./hats";
import { hueShift, type Mood, type MoodStyle, type RGB, rgba } from "./palette";

export interface LookInput {
  stage: number;
  hat: string;
  weak: boolean;
}

export interface Frame {
  /** Seconds since start (drives all motion). */
  t: number;
  /** Seconds since the mood last changed. */
  moodAge: number;
  mood: Mood;
  style: MoodStyle;
  look: { x: number; y: number };
  open: number;
  /** Centre of the bell. */
  x: number;
  y: number;
  appearance: LookInput;
  /** An emote playing right now, with progress 0..1. */
  emote: { id: string; p: number } | null;
}

const BELL_HALF_WIDTH = 34;
const DOME = 40;
const STAGE_SCALE = [0.92, 1.0, 1.06, 1.1];

function bellPath(ctx: CanvasRenderingContext2D, x: number, y: number, halfWidth: number, grow: number) {
  const w = halfWidth + grow;
  ctx.beginPath();
  ctx.moveTo(x - w, y + 12);
  ctx.bezierCurveTo(x - w, y - DOME - grow, x + w, y - DOME - grow, x + w, y + 12);
  const lobes = 6; // scalloped rim
  for (let i = 0; i < lobes; i++) {
    const x0 = x + w - (i * 2 * w) / lobes;
    const x1 = x0 - (2 * w) / lobes;
    ctx.quadraticCurveTo((x0 + x1) / 2, y + 18 + grow, x1, y + 12);
  }
  ctx.closePath();
}

const easeInOut = (p: number) => (p < 0.5 ? 2 * p * p : 1 - (-2 * p + 2) ** 2 / 2);

export function drawGlowby(ctx: CanvasRenderingContext2D, f: Frame) {
  const s = f.style;
  const stage = Math.max(0, Math.min(3, f.appearance.stage));
  const weak = f.appearance.weak;
  const pulseAmp = s.pulseAmp * (weak ? 0.5 : 1);
  const pulse = Math.sin(f.t * Math.PI * 2 * s.pulseHz);
  let x = f.x;
  let y = f.y + pulse * 2 * pulseAmp;
  let rotation = 0;
  let squash = 1;

  // Mood body language
  if (f.mood === "happy") y -= Math.abs(Math.sin(f.t * 5)) * 5;
  if (f.mood === "alert" && f.moodAge % 1.4 < 0.35) x += Math.sin(f.t * 55) * 1.8;
  if (f.mood === "sleepy") y += 3;
  if (f.mood === "sick") rotation += Math.sin(f.t * 2.2) * 0.06;

  // Emotes
  const e = f.emote;
  if (e) {
    const fade = 1 - e.p;
    switch (e.id) {
      case "wave":
        rotation += Math.sin(e.p * Math.PI * 6) * 0.2 * fade;
        break;
      case "spin":
        rotation += Math.PI * 2 * easeInOut(e.p);
        squash = 1 - 0.12 * Math.sin(e.p * Math.PI);
        break;
      case "dance":
        x += Math.sin(e.p * Math.PI * 4) * 7;
        y -= Math.abs(Math.sin(e.p * Math.PI * 4)) * 7;
        rotation += Math.sin(e.p * Math.PI * 4) * 0.12;
        break;
      case "heart":
      case "fireworks":
        y -= Math.sin(e.p * Math.PI) * 4;
        break;
    }
  }

  const scale = STAGE_SCALE[stage];
  ctx.save();
  ctx.translate(x, y);
  ctx.rotate(rotation);
  ctx.scale(scale, scale * squash);
  ctx.translate(-x, -y);
  if (weak) ctx.globalAlpha = 0.78; // ignored for days: a bit faded

  // Soft glow behind the body
  const glow = ctx.createRadialGradient(x, y - 6, 8, x, y - 6, 62);
  glow.addColorStop(0, rgba(s.halo, s.haloAlpha * (stage >= 1 ? 1.3 : 1)));
  glow.addColorStop(1, rgba(s.halo, 0));
  ctx.fillStyle = glow;
  ctx.fillRect(x - 64, y - 70, 128, 136);

  // Aurora: colours slowly travel around the colour wheel
  const aurora = stage >= 3 && f.mood !== "alert" && f.mood !== "sick";
  const hue = aurora ? Math.sin(f.t * 0.5) * 40 : 0;
  const tint = (c: RGB) => (aurora ? hueShift(c, hue) : c);

  // Tentacles: thin wavy strands, two frilly ones in the middle
  ctx.lineCap = "round";
  const wave = s.wave * (weak ? 0.6 : 1);
  const sway = f.t * (1.4 + s.pulseHz);
  const extra = stage >= 2 ? 10 : 0;
  for (let i = 0; i < 5; i++) {
    const tx = x - 20 + i * 10;
    const length = 50 + (i % 2) * 8 + extra;
    ctx.strokeStyle = rgba(tint(s.tentacle), 0.95);
    ctx.lineWidth = 2.4;
    ctx.beginPath();
    ctx.moveTo(tx, y + 14);
    for (let k = 1; k <= 10; k++) {
      const amp = (1.1 + k * 0.42) * wave;
      ctx.lineTo(tx + Math.sin(sway + k * 0.7 + i * 1.3) * amp, y + 14 + (k / 10) * length);
    }
    ctx.stroke();
  }
  const armLength = stage >= 1 ? 8 : 6;
  for (const [color, width] of [
    [rgba([255, 255, 255], 0.35), 4.5],
    [rgba(tint(s.tentacle), 0.75), 3],
  ] as const) {
    ctx.strokeStyle = color;
    ctx.lineWidth = width;
    for (const side of [-1, 1]) {
      ctx.beginPath();
      ctx.moveTo(x + side * 4, y + 14);
      for (let k = 1; k <= armLength; k++) {
        ctx.lineTo(x + side * 4 + Math.sin(sway * 1.3 + k + side) * 2.5 * wave, y + 14 + k * 5);
      }
      ctx.stroke();
    }
  }

  // The bell: outline, body, lighter inner dome, shine
  const halfWidth = BELL_HALF_WIDTH * (1 + pulse * 0.035 * pulseAmp);
  ctx.fillStyle = rgba(tint(s.edge));
  bellPath(ctx, x, y, halfWidth, 2.2);
  ctx.fill();
  if (aurora) {
    const g = ctx.createLinearGradient(x - halfWidth, y - 30, x + halfWidth, y + 12);
    g.addColorStop(0, rgba(hueShift(s.bell, hue - 35)));
    g.addColorStop(0.5, rgba(hueShift(s.bell, hue)));
    g.addColorStop(1, rgba(hueShift(s.bell, hue + 35)));
    ctx.fillStyle = g;
  } else {
    ctx.fillStyle = rgba(s.bell);
  }
  bellPath(ctx, x, y, halfWidth, 0);
  ctx.fill();
  ctx.fillStyle = rgba(tint(s.inner), 0.5);
  bellPath(ctx, x, y - 3, halfWidth - 8, -2);
  ctx.fill();

  // Lantern: warm light inside the bell
  if (stage >= 1) {
    const flicker = 0.85 + Math.sin(f.t * 3.1) * 0.08 + Math.sin(f.t * 7.3) * 0.04;
    const lamp = ctx.createRadialGradient(x, y - 9, 1, x, y - 9, 22);
    lamp.addColorStop(0, `rgba(255,236,170,${0.9 * flicker})`);
    lamp.addColorStop(0.45, `rgba(255,243,196,${0.45 * flicker})`);
    lamp.addColorStop(1, "rgba(255,243,196,0)");
    ctx.fillStyle = lamp;
    ctx.beginPath();
    ctx.arc(x, y - 9, 22, 0, Math.PI * 2);
    ctx.fill();
  }

  ctx.fillStyle = "rgba(255,255,255,0.55)";
  ctx.beginPath();
  ctx.ellipse(x - 14, y - 17, 7.5, 4, -0.2, 0, Math.PI * 2);
  ctx.fill();

  // Crown of spots (glowing from Starlit on)
  const spots: [number, number][] =
    stage >= 2 ? [[-14, -18], [-7, -23.5], [0, -25.5], [7, -23.5], [14, -18]] : [[-8, -22], [0, -24.5], [8, -22]];
  for (const [dx, dy] of spots) {
    if (stage >= 2) {
      ctx.fillStyle = "rgba(255,248,220,0.5)";
      ctx.beginPath();
      ctx.arc(x + dx, y + dy, 4, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.fillStyle = stage >= 2 ? "rgba(255,250,235,0.95)" : rgba(s.edge, 0.35);
    ctx.beginPath();
    ctx.arc(x + dx, y + dy, stage >= 2 ? 2.4 : 2.1, 0, Math.PI * 2);
    ctx.fill();
  }

  // Lantern rim lights
  if (stage >= 1) {
    for (let i = 0; i < 6; i++) {
      const lx = x - halfWidth + (halfWidth * 2 * (i + 0.5)) / 6;
      const glowA = 0.6 + Math.sin(f.t * 2.4 + i) * 0.35;
      ctx.fillStyle = `rgba(255,236,170,${glowA})`;
      ctx.beginPath();
      ctx.arc(lx, y + 15, 2.1, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  drawFace(ctx, x, y - 2, { mood: f.mood, look: f.look, open: f.open });
  if (f.appearance.hat) drawHat(ctx, f.appearance.hat, x, y - 28, f.t);
  ctx.restore();

  if (stage >= 2) drawSparkles(ctx, f, x, y, stage);
  drawMoodExtras(ctx, f, x, y);
  if (e) drawEmoteExtras(ctx, e, x, y);
}

/** Starlit and Aurora: a few tiny stars drifting around Glowby. */
function drawSparkles(ctx: CanvasRenderingContext2D, f: Frame, x: number, y: number, stage: number) {
  const count = stage >= 3 ? 6 : 3;
  for (let i = 0; i < count; i++) {
    const a = f.t * 0.4 + (i * Math.PI * 2) / count;
    const sx = x + Math.cos(a) * (46 + (i % 2) * 6);
    const sy = y + 6 + Math.sin(a) * 30;
    const tw = 0.4 + 0.6 * Math.abs(Math.sin(f.t * 2 + i));
    ctx.fillStyle = `rgba(255,250,225,${tw})`;
    ctx.beginPath();
    ctx.arc(sx, sy, 1.3, 0, Math.PI * 2);
    ctx.fill();
  }
  if (stage >= 3) {
    // trail of light drifting down from the tentacles
    for (let i = 0; i < 5; i++) {
      const p = (f.t * 0.3 + i / 5) % 1;
      ctx.fillStyle = `rgba(200,220,255,${0.6 * (1 - p)})`;
      ctx.beginPath();
      ctx.arc(x - 16 + i * 8 + Math.sin(f.t + i) * 3, y + 40 + p * 40, 1.4, 0, Math.PI * 2);
      ctx.fill();
    }
  }
}

function heart(ctx: CanvasRenderingContext2D, x: number, y: number, size: number) {
  ctx.beginPath();
  ctx.moveTo(x, y + size * 0.35);
  ctx.bezierCurveTo(x - size, y - size * 0.4, x - size * 0.4, y - size, x, y - size * 0.45);
  ctx.bezierCurveTo(x + size * 0.4, y - size, x + size, y - size * 0.4, x, y + size * 0.35);
  ctx.fill();
}

function drawEmoteExtras(ctx: CanvasRenderingContext2D, e: { id: string; p: number }, x: number, y: number) {
  const fade = 1 - e.p;
  ctx.save();
  switch (e.id) {
    case "heart": {
      ctx.fillStyle = `rgba(240,110,150,${fade})`;
      for (let i = 0; i < 3; i++) {
        const p = Math.min(1, e.p * 1.3 + i * 0.12);
        heart(ctx, x - 22 + i * 22, y - 26 - p * 30, 6 + i);
      }
      break;
    }
    case "dance": {
      ctx.fillStyle = `rgba(127,119,221,${fade})`;
      ctx.font = "600 16px 'Segoe UI Symbol', 'Segoe UI', sans-serif";
      ctx.textAlign = "center";
      ctx.fillText("♪", x - 40, y - 20 - e.p * 18);
      ctx.fillText("♫", x + 42, y - 10 - e.p * 24);
      break;
    }
    case "fireworks": {
      const colors = ["#FAC775", "#ED93B1", "#7F9BFF", "#5DCAA5"];
      const bursts: [number, number, number][] = [[-42, -30, 0], [40, -36, 0.15], [0, -50, 0.3]];
      ctx.lineWidth = 1.8;
      ctx.lineCap = "round";
      bursts.forEach(([bx, by, delay], b) => {
        const p = (e.p - delay) / (1 - delay);
        if (p <= 0) return;
        const r = 4 + p * 16;
        ctx.strokeStyle = colors[b % colors.length];
        ctx.globalAlpha = Math.max(0, 1 - p);
        for (let i = 0; i < 8; i++) {
          const a = (i * Math.PI) / 4;
          ctx.beginPath();
          ctx.moveTo(x + bx + Math.cos(a) * r * 0.5, y + by + Math.sin(a) * r * 0.5);
          ctx.lineTo(x + bx + Math.cos(a) * r, y + by + Math.sin(a) * r);
          ctx.stroke();
        }
      });
      break;
    }
    case "wave": {
      ctx.strokeStyle = `rgba(127,155,255,${fade})`;
      ctx.lineWidth = 1.5;
      for (let i = 1; i <= 2; i++) {
        ctx.beginPath();
        ctx.arc(x + 40, y - 20, i * 6, -0.8, 0.8);
        ctx.stroke();
      }
      break;
    }
  }
  ctx.restore();
}

function drawMoodExtras(ctx: CanvasRenderingContext2D, f: Frame, x: number, y: number) {
  const edge = x + BELL_HALF_WIDTH;
  switch (f.mood) {
    case "working": {
      // little bubbles rising: busy swimming
      ctx.strokeStyle = rgba(f.style.edge, 0.55);
      ctx.lineWidth = 1.2;
      for (let j = 0; j < 3; j++) {
        const phase = (f.t * 0.7 + j / 3) % 1;
        const side = j % 2 ? 1 : -1;
        const bx = x + side * (BELL_HALF_WIDTH + 9 + Math.sin(f.t * 3 + j) * 2.5);
        const by = y + 26 - phase * 62;
        ctx.globalAlpha = 1 - phase;
        ctx.beginPath();
        ctx.arc(bx, by, 1.8 + j * 0.7, 0, Math.PI * 2);
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
      break;
    }
    case "happy": {
      ctx.strokeStyle = "#F2B13C";
      ctx.lineWidth = 1.8;
      ctx.lineCap = "round";
      for (let i = 0; i < 4; i++) {
        const a = f.t * 0.9 + i * (Math.PI / 2);
        const sx = x + Math.cos(a) * 52;
        const sy = y - 6 + Math.sin(a) * 34;
        const r = 3.2 + Math.sin(f.t * 6 + i) * 1.2;
        ctx.beginPath();
        ctx.moveTo(sx - r, sy);
        ctx.lineTo(sx + r, sy);
        ctx.moveTo(sx, sy - r);
        ctx.lineTo(sx, sy + r);
        ctx.stroke();
      }
      break;
    }
    case "alert": {
      if (Math.sin(f.t * 8) > -0.3) {
        ctx.fillStyle = "#E24B4A";
        ctx.font = "700 24px 'Segoe UI', sans-serif";
        ctx.textAlign = "center";
        ctx.fillText("!", edge + 14, y - 16);
      }
      break;
    }
    case "sleepy": {
      ctx.fillStyle = "#7F77DD";
      ctx.textAlign = "center";
      for (let i = 0; i < 2; i++) {
        const p = (f.t * 0.35 + i * 0.5) % 1;
        ctx.globalAlpha = 1 - p;
        ctx.font = `600 ${11 + i * 4}px 'Segoe UI', sans-serif`;
        ctx.fillText(i ? "Z" : "z", edge + 6 + p * 10, y - 18 - p * 26);
      }
      ctx.globalAlpha = 1;
      break;
    }
    case "sick": {
      const bob = Math.sin(f.t * 3) * 1.5;
      ctx.fillStyle = "#8FC8F0";
      ctx.beginPath();
      ctx.moveTo(edge - 4, y - 24 + bob);
      ctx.quadraticCurveTo(edge + 1, y - 15 + bob, edge - 4, y - 13 + bob);
      ctx.quadraticCurveTo(edge - 9, y - 15 + bob, edge - 4, y - 24 + bob);
      ctx.fill();
      break;
    }
    case "idle":
      break;
  }
}
