// Glowby, drawn entirely in code with the Canvas 2D API.
// A jellyfish: a pulsing bell (dome), a soft glow, waving tentacles, a face.

import { drawFace } from "./face";
import { type Mood, type MoodStyle, rgba } from "./palette";

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
}

const BELL_HALF_WIDTH = 34;
const DOME = 40;

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

export function drawGlowby(ctx: CanvasRenderingContext2D, f: Frame) {
  const s = f.style;
  const pulse = Math.sin(f.t * Math.PI * 2 * s.pulseHz);
  let x = f.x;
  let y = f.y + pulse * 2 * s.pulseAmp;

  // Mood body language
  if (f.mood === "happy") y -= Math.abs(Math.sin(f.t * 5)) * 5;
  if (f.mood === "alert" && f.moodAge % 1.4 < 0.35) x += Math.sin(f.t * 55) * 1.8;
  if (f.mood === "sleepy") y += 3;

  ctx.save();
  if (f.mood === "sick") {
    ctx.translate(x, y);
    ctx.rotate(Math.sin(f.t * 2.2) * 0.06);
    ctx.translate(-x, -y);
  }

  // Soft glow behind the body
  const glow = ctx.createRadialGradient(x, y - 6, 8, x, y - 6, 62);
  glow.addColorStop(0, rgba(s.halo, s.haloAlpha));
  glow.addColorStop(1, rgba(s.halo, 0));
  ctx.fillStyle = glow;
  ctx.fillRect(x - 64, y - 70, 128, 136);

  // Tentacles: thin wavy strands, two frilly ones in the middle
  ctx.lineCap = "round";
  const sway = f.t * (1.4 + s.pulseHz);
  for (let i = 0; i < 5; i++) {
    const tx = x - 20 + i * 10;
    const length = 50 + (i % 2) * 8;
    ctx.strokeStyle = rgba(s.tentacle, 0.95);
    ctx.lineWidth = 2.4;
    ctx.beginPath();
    ctx.moveTo(tx, y + 14);
    for (let k = 1; k <= 10; k++) {
      const amp = (1.1 + k * 0.42) * s.wave;
      ctx.lineTo(tx + Math.sin(sway + k * 0.7 + i * 1.3) * amp, y + 14 + (k / 10) * length);
    }
    ctx.stroke();
  }
  ctx.strokeStyle = rgba([255, 255, 255], 0.35);
  ctx.lineWidth = 4.5;
  for (const side of [-1, 1]) {
    ctx.beginPath();
    ctx.moveTo(x + side * 4, y + 14);
    for (let k = 1; k <= 6; k++) {
      ctx.lineTo(x + side * 4 + Math.sin(sway * 1.3 + k + side) * 2.5 * s.wave, y + 14 + k * 5);
    }
    ctx.stroke();
  }
  ctx.strokeStyle = rgba(s.tentacle, 0.75);
  ctx.lineWidth = 3;
  for (const side of [-1, 1]) {
    ctx.beginPath();
    ctx.moveTo(x + side * 4, y + 14);
    for (let k = 1; k <= 6; k++) {
      ctx.lineTo(x + side * 4 + Math.sin(sway * 1.3 + k + side) * 2.5 * s.wave, y + 14 + k * 5);
    }
    ctx.stroke();
  }

  // The bell: outline, body, lighter inner dome, shine, little crown of spots
  const halfWidth = BELL_HALF_WIDTH * (1 + pulse * 0.035 * s.pulseAmp);
  ctx.fillStyle = rgba(s.edge);
  bellPath(ctx, x, y, halfWidth, 2.2);
  ctx.fill();
  ctx.fillStyle = rgba(s.bell);
  bellPath(ctx, x, y, halfWidth, 0);
  ctx.fill();
  ctx.fillStyle = rgba(s.inner, 0.5);
  bellPath(ctx, x, y - 3, halfWidth - 8, -2);
  ctx.fill();
  ctx.fillStyle = "rgba(255,255,255,0.55)";
  ctx.beginPath();
  ctx.ellipse(x - 14, y - 17, 7.5, 4, -0.2, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = rgba(s.edge, 0.35);
  for (const [dx, dy] of [[-8, -22], [0, -24.5], [8, -22]]) {
    ctx.beginPath();
    ctx.arc(x + dx, y + dy, 2.1, 0, Math.PI * 2);
    ctx.fill();
  }

  drawFace(ctx, x, y - 2, { mood: f.mood, look: f.look, open: f.open });
  ctx.restore();

  drawMoodExtras(ctx, f, x, y);
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
