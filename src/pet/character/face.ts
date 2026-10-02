// Glowby's face: eyes, cheeks and mouth for each mood, plus a few short-lived
// expressions for emotes and petting (laughing, a wink, shy, star eyes, love).

import { INK, type Mood } from "./palette";

/** Expressions that override the mood's face for a moment. */
export type Expr = "happy" | "love" | "laugh" | "wink" | "shy" | "star";

export interface FaceInput {
  mood: Mood;
  /** Where the eyes look, each axis -1..1. */
  look: { x: number; y: number };
  /** 1 = eyes open, 0 = closed (blinking). */
  open: number;
  expr?: Expr | null;
}

const TAU = Math.PI * 2;

export function ellipse(ctx: CanvasRenderingContext2D, x: number, y: number, rx: number, ry: number) {
  ctx.beginPath();
  ctx.ellipse(x, y, Math.max(rx, 0.1), Math.max(ry, 0.1), 0, 0, TAU);
  ctx.fill();
}

export function star5(ctx: CanvasRenderingContext2D, x: number, y: number, r: number) {
  ctx.beginPath();
  for (let i = 0; i < 10; i++) {
    const a = -Math.PI / 2 + (i * Math.PI) / 5;
    const rr = i % 2 ? r * 0.45 : r;
    ctx.lineTo(x + Math.cos(a) * rr, y + Math.sin(a) * rr);
  }
  ctx.closePath();
  ctx.fill();
}

/** "///" blush lines, the anime sign for being flustered. */
export function blushLines(ctx: CanvasRenderingContext2D, x: number, y: number) {
  ctx.save();
  ctx.strokeStyle = "rgba(225,90,120,0.75)";
  ctx.lineWidth = 1.1;
  for (let i = 0; i < 3; i++) {
    ctx.beginPath();
    ctx.moveTo(x - 3 + i * 2.6, y + 2);
    ctx.lineTo(x - 1 + i * 2.6, y - 2);
    ctx.stroke();
  }
  ctx.restore();
}

export function drawFace(ctx: CanvasRenderingContext2D, x: number, y: number, f: FaceInput) {
  const gap = 11;
  const lx = f.look.x * 2.8;
  const ly = f.look.y * 2.2;
  const expr = f.expr ?? null;
  // "happy" and "love" look like the happy mood; the others draw their own eyes.
  const mood: Mood = expr === "happy" || expr === "love" ? "happy" : f.mood;
  const flustered = expr === "love" || expr === "shy";

  ctx.fillStyle = flustered ? "rgba(237,96,130,0.6)" : "rgba(237,110,140,0.38)";
  ellipse(ctx, x - gap - 7, y + 6, flustered ? 6 : 5, flustered ? 3.6 : 3);
  ellipse(ctx, x + gap + 7, y + 6, flustered ? 6 : 5, flustered ? 3.6 : 3);
  if (expr === "shy") {
    blushLines(ctx, x - gap - 7, y + 6);
    blushLines(ctx, x + gap + 7, y + 6);
  }

  ctx.fillStyle = INK;
  ctx.strokeStyle = INK;
  ctx.lineWidth = 2.4;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";

  for (const side of [-1, 1]) {
    const ex = x + side * gap;
    if (expr === "laugh") {
      // > <
      ctx.beginPath();
      ctx.moveTo(ex - side * 3.5, y - 3.2);
      ctx.lineTo(ex + side * 3, y);
      ctx.lineTo(ex - side * 3.5, y + 3.2);
      ctx.stroke();
      continue;
    }
    if (expr === "wink" && side === 1) {
      ctx.beginPath();
      ctx.moveTo(ex - 4.2, y + 1.2);
      ctx.quadraticCurveTo(ex, y - 5.5, ex + 4.2, y + 1.2);
      ctx.stroke();
      continue;
    }
    if (expr === "shy") {
      // glancing down and away
      ellipse(ctx, ex - 1.8, y + 1.6, 3.2, 3.6 * f.open);
      continue;
    }
    if (expr === "star") {
      ellipse(ctx, ex, y, 4.8, 5.4);
      ctx.fillStyle = "#FFD34D";
      star5(ctx, ex, y - 0.3, 3.9);
      ctx.fillStyle = INK;
      continue;
    }
    switch (expr === "wink" ? "idle" : mood) {
      case "idle":
        ellipse(ctx, ex + lx, y + ly, 3.8, 4.8 * f.open);
        if (f.open > 0.6) {
          ctx.fillStyle = "#fff";
          ellipse(ctx, ex + lx + 1.2, y + ly - 1.8, 1.25, 1.25);
          ctx.fillStyle = INK;
        }
        break;
      case "working": // focused: half-lidded, looking down at the "work"
        ellipse(ctx, ex + lx * 0.6, y + 1.8, 3.6, 2.7 * Math.max(f.open, 0.2));
        ctx.beginPath();
        ctx.moveTo(ex - 5, y - 1.6);
        ctx.lineTo(ex + 5, y - 1.6);
        ctx.stroke();
        break;
      case "happy": // ^ ^
        ctx.beginPath();
        ctx.moveTo(ex - 4.2, y + 1.2);
        ctx.quadraticCurveTo(ex, y - 5.5, ex + 4.2, y + 1.2);
        ctx.stroke();
        break;
      case "alert": // wide open
        ellipse(ctx, ex, y, 4.8, 5.6);
        ctx.fillStyle = "#fff";
        ellipse(ctx, ex + 1.5, y - 2, 1.8, 1.8);
        ctx.fillStyle = INK;
        break;
      case "sleepy": // closed, curved down
        ctx.beginPath();
        ctx.moveTo(ex - 4.2, y);
        ctx.quadraticCurveTo(ex, y + 3.8, ex + 4.2, y);
        ctx.stroke();
        break;
      case "sick": // little spirals
        ctx.lineWidth = 1.6;
        ctx.beginPath();
        for (let a = 0; a < 12; a += 0.35) {
          const r = a * 0.42;
          const px = ex + Math.cos(side * a) * r;
          const py = y + Math.sin(side * a) * r;
          if (a === 0) ctx.moveTo(px, py);
          else ctx.lineTo(px, py);
        }
        ctx.stroke();
        ctx.lineWidth = 2.4;
        break;
    }
  }

  ctx.beginPath();
  if (expr === "laugh" || expr === "star") {
    // big open smile
    ctx.arc(x, y + 5, 6, 0, Math.PI);
    ctx.closePath();
    ctx.fill();
    ctx.fillStyle = "#F08FA6";
    ellipse(ctx, x, y + 9, 3, 1.6);
    return;
  }
  if (expr === "wink") {
    ctx.arc(x, y + 5.5, 4, 0.1 * Math.PI, 0.9 * Math.PI);
    ctx.stroke();
    ctx.fillStyle = "#F08FA6";
    ellipse(ctx, x + 1.5, y + 9.4, 1.8, 1.4);
    return;
  }
  if (expr === "shy") {
    ctx.lineWidth = 1.6;
    ctx.moveTo(x - 3.5, y + 8);
    ctx.quadraticCurveTo(x - 1.7, y + 6.5, x, y + 8);
    ctx.quadraticCurveTo(x + 1.7, y + 9.5, x + 3.5, y + 8);
    ctx.stroke();
    return;
  }
  switch (mood) {
    case "idle":
      ctx.arc(x, y + 5.5, 3.2, 0.15 * Math.PI, 0.85 * Math.PI);
      ctx.stroke();
      break;
    case "working":
      ctx.moveTo(x - 2.6, y + 8);
      ctx.lineTo(x + 2.6, y + 8);
      ctx.stroke();
      break;
    case "happy":
      ctx.arc(x, y + 5.5, 5, 0, Math.PI);
      ctx.closePath();
      ctx.fill();
      ctx.fillStyle = "#F08FA6";
      ellipse(ctx, x, y + 8.8, 2.4, 1.3);
      break;
    case "alert":
      ctx.arc(x, y + 9, 2.6, 0, TAU);
      ctx.stroke();
      break;
    case "sleepy":
      ellipse(ctx, x, y + 8, 2, 2.4);
      break;
    case "sick":
      ctx.moveTo(x - 5.5, y + 9);
      for (let i = 1; i <= 4; i++) ctx.lineTo(x - 5.5 + i * 2.75, y + 9 + (i % 2 ? -1.7 : 1.7));
      ctx.stroke();
      break;
  }
}
