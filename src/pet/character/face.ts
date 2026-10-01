// Glowby's face: eyes, cheeks and mouth for each mood.

import { INK, type Mood } from "./palette";

export interface FaceInput {
  mood: Mood;
  /** Where the eyes look, each axis -1..1. */
  look: { x: number; y: number };
  /** 1 = eyes open, 0 = closed (blinking). */
  open: number;
}

const TAU = Math.PI * 2;

function ellipse(ctx: CanvasRenderingContext2D, x: number, y: number, rx: number, ry: number) {
  ctx.beginPath();
  ctx.ellipse(x, y, Math.max(rx, 0.1), Math.max(ry, 0.1), 0, 0, TAU);
  ctx.fill();
}

export function drawFace(ctx: CanvasRenderingContext2D, x: number, y: number, f: FaceInput) {
  const gap = 11;
  const lx = f.look.x * 2.8;
  const ly = f.look.y * 2.2;

  ctx.fillStyle = "rgba(237,110,140,0.38)";
  ellipse(ctx, x - gap - 7, y + 6, 5, 3);
  ellipse(ctx, x + gap + 7, y + 6, 5, 3);

  ctx.fillStyle = INK;
  ctx.strokeStyle = INK;
  ctx.lineWidth = 2.4;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";

  for (const side of [-1, 1]) {
    const ex = x + side * gap;
    switch (f.mood) {
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
  switch (f.mood) {
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
