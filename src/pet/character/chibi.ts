// Glowby's anime-style friends, drawn in code like Glowby himself. "Chibi" is
// the anime style with a big head and a tiny body. All five are original
// designs: Neko (a cat), Kitsune (a fox spirit), Slime, Mini Ninja, Mini Robot.
//
// Unlike the jellyfish they have arms, so they can wave, cheer, make a peace
// sign or hide their blushing cheeks. They share all of Glowby's body language
// (bobbing, moods, emotes) through bodyMotion().
//
// Coordinates: (x, y) is the centre of the head; the body hangs below it.

import { blushLines, ellipse, type Expr, star5 } from "./face";
import { bodyMotion, drawEmoteExtras, drawMoodExtras, drawSparkles, expressionFor, type Frame, STAGE_SCALE } from "./glowby";
import { drawHat } from "./hats";
import { COLORWAYS, INK, type Mood, rgba } from "./palette";

export type Kind = "cat" | "fox" | "slime" | "ninja" | "robot";

export interface Species {
  id: string;
  name: string;
  kind: Kind;
  /** fur / outfit / plating */
  main: string;
  /** outlines and shading */
  dark: string;
  /** muzzle, belly, skin */
  light: string;
  /** inner ears, scarf, lights */
  accent: string;
  iris: string;
}

export const SPECIES: Record<string, Species> = {
  neko: { id: "neko", name: "Neko", kind: "cat", main: "#FFE1C2", dark: "#C4885A", light: "#FFF7EE", accent: "#F5A3B5", iris: "#4F86E8" },
  slime: { id: "slime", name: "Slime", kind: "slime", main: "", dark: "", light: "", accent: "", iris: INK },
  kitsune: { id: "kitsune", name: "Kitsune", kind: "fox", main: "#F49A45", dark: "#AE541D", light: "#FFF4E6", accent: "#FFD9B8", iris: "#C2334D" },
  ninja: { id: "ninja", name: "Mini Ninja", kind: "ninja", main: "#3B4268", dark: "#22273F", light: "#FFE3CC", accent: "#D8423A", iris: "#3E7BD6" },
  mecha: { id: "mecha", name: "Mini Robot", kind: "robot", main: "#E4E9F2", dark: "#7D879B", light: "#F7F9FC", accent: "#55D0E8", iris: "#55D0E8" },
};

export const HEAD_R = 27;
const TAU = Math.PI * 2;
/** Where the top of the head is, for hats (relative to the head centre). */
export const HAT_TOP: Record<Kind, number> = { cat: -27, fox: -27, slime: -24, ninja: -33, robot: -25 };

interface Pose {
  /** Arm angles in radians: 0 = hanging down, π/2 = straight out, π = straight up. */
  armL: number;
  armR: number;
  peace: boolean;
  /** How fast the tail wags. */
  wag: number;
}

function poseFor(f: Frame): Pose {
  const t = f.t;
  let armL = 0.35 + Math.sin(t * 2) * 0.05;
  let armR = 0.35 + Math.sin(t * 2 + 1) * 0.05;
  let peace = false;
  let wag = 1.6;
  switch (f.mood) {
    case "working": // typing away
      armL = 0.8 + Math.sin(t * 14) * 0.15;
      armR = 0.8 + Math.sin(t * 14 + Math.PI) * 0.15;
      wag = 2;
      break;
    case "happy":
      armL = armR = 1.9 + Math.sin(t * 8) * 0.25;
      wag = 5;
      break;
    case "alert": // startled, hands up
      armL = armR = 1.3;
      wag = 0.4;
      break;
    case "sleepy":
      armL = armR = 0.12;
      wag = 0.4;
      break;
    case "sick":
      armL = armR = 0.22;
      wag = 0.3;
      break;
  }
  if (f.petting > 0.05) {
    armL = armR = 0.6;
    wag = 8;
  }
  const e = f.emote;
  if (e) {
    // ease into the pose and back out
    const k = Math.min(1, Math.sin(e.p * Math.PI) * 3);
    const to = (from: number, target: number) => from + (target - from) * k;
    switch (e.id) {
      case "wave":
        armR = to(armR, 2.25 + Math.sin(e.p * Math.PI * 8) * 0.4);
        wag = 5;
        break;
      case "heart":
        armL = to(armL, 1.05);
        armR = to(armR, 1.05);
        break;
      case "dance":
        armL = 1.2 + Math.sin(e.p * Math.PI * 4) * 1.1;
        armR = 1.2 - Math.sin(e.p * Math.PI * 4) * 1.1;
        wag = 7;
        break;
      case "fireworks":
      case "cheer":
        armL = to(armL, 2.5 + Math.sin(e.p * Math.PI * 8) * 0.3);
        armR = to(armR, 2.5 - Math.sin(e.p * Math.PI * 8) * 0.3);
        wag = 7;
        break;
      case "jump":
        armL = to(armL, 2.3);
        armR = to(armR, 2.3);
        break;
      case "laugh":
        armL = to(armL, 0.95);
        armR = to(armR, 0.95);
        break;
      case "peace":
        armR = to(armR, 2.2);
        peace = k > 0.6;
        break;
      case "shy": // paws on the cheeks
        armL = to(armL, 2.75);
        armR = to(armR, 2.75);
        break;
      case "sparkle":
        armL = to(armL, 1.75);
        armR = to(armR, 1.75);
        wag = 6;
        break;
      case "spin":
        armL = armR = 1.57;
        break;
      case "idle-stretch":
        armL = to(armL, 3.0);
        armR = to(armR, 3.0);
        break;
      case "idle-wag":
        wag = 10;
        break;
    }
  }
  return { armL, armR, peace, wag };
}

function moodExpr(mood: Mood): "normal" | "focus" | "happy" | "alert" | "sleepy" | "sick" {
  switch (mood) {
    case "working":
      return "focus";
    case "idle":
      return "normal";
    default:
      return mood;
  }
}

// ---------------------------------------------------------------- faces

/** One big anime eye: dark outline, coloured iris, pupil, two white highlights. */
function animeEye(ctx: CanvasRenderingContext2D, ex: number, ey: number, look: { x: number; y: number }, open: number, iris: string, scale = 1) {
  const rx = 5.4 * scale;
  const ry = 7 * scale * open;
  ctx.fillStyle = INK;
  ellipse(ctx, ex, ey, rx, ry);
  if (open < 0.35) return;
  const lx = look.x * 1.3;
  const ly = look.y * 1.3;
  ctx.fillStyle = iris;
  ellipse(ctx, ex + lx, ey + 1.6 * scale + ly, rx * 0.76, ry * 0.6);
  ctx.fillStyle = INK;
  ellipse(ctx, ex + lx, ey + 1.8 * scale + ly, rx * 0.36, ry * 0.3);
  ctx.fillStyle = "#fff";
  ellipse(ctx, ex - 1.9 * scale + lx * 0.5, ey - 2.8 * scale, 2 * scale, 2.2 * scale);
  ellipse(ctx, ex + 2.1 * scale + lx * 0.5, ey + 3 * scale, 0.95 * scale, 0.95 * scale);
}

function happyArc(ctx: CanvasRenderingContext2D, ex: number, ey: number) {
  ctx.beginPath();
  ctx.moveTo(ex - 4.6, ey + 1.5);
  ctx.quadraticCurveTo(ex, ey - 5.5, ex + 4.6, ey + 1.5);
  ctx.stroke();
}

/** Eyes, blush and mouth in the anime style. `catMouth` draws the little "ω". */
function animeFace(ctx: CanvasRenderingContext2D, x: number, y: number, f: Frame, iris: string, catMouth: boolean) {
  const expr: Expr | ReturnType<typeof moodExpr> = expressionFor(f) ?? moodExpr(f.mood);
  const gap = 11.5;
  const flustered = expr === "love" || expr === "shy";
  ctx.fillStyle = flustered ? "rgba(240,100,135,0.55)" : "rgba(240,120,150,0.35)";
  ellipse(ctx, x - 17, y + 7, flustered ? 6 : 5, flustered ? 3.6 : 3);
  ellipse(ctx, x + 17, y + 7, flustered ? 6 : 5, flustered ? 3.6 : 3);
  if (expr === "shy") {
    blushLines(ctx, x - 17, y + 7);
    blushLines(ctx, x + 17, y + 7);
  }
  ctx.strokeStyle = INK;
  ctx.fillStyle = INK;
  ctx.lineWidth = 2.3;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  for (const side of [-1, 1]) {
    const ex = x + side * gap;
    switch (expr) {
      case "normal":
        animeEye(ctx, ex, y, f.look, f.open, iris);
        break;
      case "focus":
        animeEye(ctx, ex, y + 1, { x: f.look.x * 0.5, y: 0.6 }, Math.min(f.open, 0.62), iris);
        ctx.beginPath(); // determined eyebrows
        ctx.moveTo(ex - side * 5, y - 7);
        ctx.lineTo(ex + side * 4, y - 5.5);
        ctx.stroke();
        break;
      case "alert":
        animeEye(ctx, ex, y, { x: 0, y: 0 }, 1.12, iris, 1.05);
        break;
      case "happy":
      case "love":
        happyArc(ctx, ex, y);
        break;
      case "laugh": // > <
        ctx.beginPath();
        ctx.moveTo(ex - side * 4, y - 3.5);
        ctx.lineTo(ex + side * 3.5, y);
        ctx.lineTo(ex - side * 4, y + 3.5);
        ctx.stroke();
        break;
      case "wink":
        if (side === 1) happyArc(ctx, ex, y);
        else animeEye(ctx, ex, y, f.look, f.open, iris);
        break;
      case "shy":
        animeEye(ctx, ex, y + 1, { x: -0.9, y: 0.8 }, 0.75 * f.open, iris, 0.92);
        break;
      case "star":
        ctx.fillStyle = INK;
        ellipse(ctx, ex, y, 5.4, 6.6);
        ctx.fillStyle = "#FFD34D";
        star5(ctx, ex, y, 4.4);
        ctx.fillStyle = "#fff";
        ellipse(ctx, ex - 2, y - 3, 1.2, 1.2);
        ctx.fillStyle = INK;
        break;
      case "sleepy":
        ctx.beginPath();
        ctx.moveTo(ex - 4.6, y + 1);
        ctx.quadraticCurveTo(ex, y + 5, ex + 4.6, y + 1);
        ctx.stroke();
        break;
      case "sick":
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        for (let a = 0; a < 12; a += 0.35) {
          const r = a * 0.45;
          const px = ex + Math.cos(side * a) * r;
          const py = y + Math.sin(side * a) * r;
          if (a === 0) ctx.moveTo(px, py);
          else ctx.lineTo(px, py);
        }
        ctx.stroke();
        ctx.lineWidth = 2.3;
        break;
    }
  }
  // mouth
  const my = y + 10;
  ctx.lineWidth = 1.8;
  ctx.fillStyle = INK;
  ctx.beginPath();
  switch (expr) {
    case "happy":
    case "laugh":
    case "star": {
      const w = expr === "laugh" ? 5.5 : 4.5;
      ctx.arc(x, my - 1, w, 0, Math.PI);
      ctx.closePath();
      ctx.fill();
      ctx.fillStyle = "#F08FA6";
      ellipse(ctx, x, my + w * 0.55, w * 0.5, w * 0.28);
      break;
    }
    case "love":
    case "normal":
      if (catMouth) {
        // ω
        ctx.moveTo(x - 4.5, my - 1.5);
        ctx.quadraticCurveTo(x - 2.2, my + 2.2, x, my - 0.8);
        ctx.quadraticCurveTo(x + 2.2, my + 2.2, x + 4.5, my - 1.5);
      } else {
        ctx.arc(x, my - 2, 3.2, 0.15 * Math.PI, 0.85 * Math.PI);
      }
      ctx.stroke();
      break;
    case "wink":
      ctx.arc(x, my - 2, 3.6, 0.1 * Math.PI, 0.9 * Math.PI);
      ctx.stroke();
      ctx.fillStyle = "#F08FA6";
      ellipse(ctx, x + 1.4, my + 1.8, 1.7, 1.3);
      break;
    case "focus":
      ctx.moveTo(x - 2.5, my);
      ctx.lineTo(x + 2.5, my);
      ctx.stroke();
      break;
    case "alert":
      ctx.arc(x, my + 0.5, 2.4, 0, TAU);
      ctx.stroke();
      break;
    case "sleepy":
      ellipse(ctx, x, my, 1.8, 2.2);
      break;
    case "shy":
    case "sick":
      ctx.moveTo(x - 4, my);
      for (let i = 1; i <= 4; i++) ctx.lineTo(x - 4 + i * 2, my + (i % 2 ? -1.4 : 1.4));
      ctx.stroke();
      break;
  }
}

/** The robot's face: glowing LED shapes on a dark visor. */
function ledFace(ctx: CanvasRenderingContext2D, x: number, y: number, f: Frame, color: string) {
  const expr = expressionFor(f) ?? moodExpr(f.mood);
  const draw = (fn: () => void) => {
    // a soft glow (wide, faint) under a crisp line
    ctx.globalAlpha = 0.35;
    ctx.lineWidth = 5;
    fn();
    ctx.globalAlpha = 1;
    ctx.lineWidth = 2.2;
    fn();
  };
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineCap = "round";
  for (const side of [-1, 1]) {
    const ex = x + side * 9;
    switch (expr) {
      case "normal":
      case "focus":
      case "alert": {
        const h = (expr === "alert" ? 9 : expr === "focus" ? 3 : 7) * Math.max(f.open, 0.15);
        const w = expr === "alert" ? 7 : 6;
        ctx.globalAlpha = 0.35;
        ctx.fillRect(ex - w / 2 - 1.5 + f.look.x, y - h / 2 - 1.5, w + 3, h + 3);
        ctx.globalAlpha = 1;
        ctx.fillRect(ex - w / 2 + f.look.x, y - h / 2, w, h);
        break;
      }
      case "happy":
      case "love":
        draw(() => {
          ctx.beginPath();
          ctx.moveTo(ex - 4, y + 1.5);
          ctx.lineTo(ex, y - 2.5);
          ctx.lineTo(ex + 4, y + 1.5);
          ctx.stroke();
        });
        break;
      case "laugh":
        draw(() => {
          ctx.beginPath();
          ctx.moveTo(ex - side * 3.5, y - 3);
          ctx.lineTo(ex + side * 3, y);
          ctx.lineTo(ex - side * 3.5, y + 3);
          ctx.stroke();
        });
        break;
      case "wink":
        if (side === 1)
          draw(() => {
            ctx.beginPath();
            ctx.moveTo(ex - 4, y + 1.5);
            ctx.lineTo(ex, y - 2.5);
            ctx.lineTo(ex + 4, y + 1.5);
            ctx.stroke();
          });
        else ctx.fillRect(ex - 3, y - 3.5, 6, 7);
        break;
      case "shy":
        ctx.fillRect(ex - 3 - 1.5, y, 6, 4);
        break;
      case "star":
        ctx.globalAlpha = 0.4;
        star5(ctx, ex, y, 6.5);
        ctx.globalAlpha = 1;
        star5(ctx, ex, y, 4.5);
        break;
      case "sleepy":
        draw(() => {
          ctx.beginPath();
          ctx.moveTo(ex - 4, y + 1);
          ctx.lineTo(ex + 4, y + 1);
          ctx.stroke();
        });
        break;
      case "sick":
        draw(() => {
          ctx.beginPath();
          ctx.moveTo(ex - 3, y - 3);
          ctx.lineTo(ex + 3, y + 3);
          ctx.moveTo(ex + 3, y - 3);
          ctx.lineTo(ex - 3, y + 3);
          ctx.stroke();
        });
        break;
    }
  }
  if (expr === "shy" || expr === "love") {
    // pink "blush pixels" under the visor
    ctx.fillStyle = "rgba(255,130,165,0.8)";
    for (const side of [-1, 1]) {
      ctx.fillRect(x + side * 17 - 3, y + 13, 2, 2);
      ctx.fillRect(x + side * 17 + 1, y + 13, 2, 2);
    }
  }
}

// ---------------------------------------------------------------- body parts

function arm(ctx: CanvasRenderingContext2D, sx: number, sy: number, side: number, angle: number, sleeve: string, hand: string, outline: string, peace: boolean) {
  // raised arms reach a bit further, so a waving paw shows beside the big head
  const len = 14 + Math.max(0, angle - 1.3) * 5.5;
  const hx = sx + side * Math.sin(angle) * len;
  const hy = sy + Math.cos(angle) * len;
  ctx.lineCap = "round";
  ctx.strokeStyle = outline;
  ctx.lineWidth = 9.5;
  ctx.beginPath();
  ctx.moveTo(sx, sy);
  ctx.lineTo(hx, hy);
  ctx.stroke();
  ctx.strokeStyle = sleeve;
  ctx.lineWidth = 7;
  ctx.stroke();
  ctx.fillStyle = hand;
  ctx.strokeStyle = outline;
  ctx.lineWidth = 1.4;
  ctx.beginPath();
  ctx.arc(hx, hy, 4.6, 0, TAU);
  ctx.fill();
  ctx.stroke();
  if (peace) {
    // two fingers up: ✌
    ctx.strokeStyle = outline;
    ctx.lineWidth = 2.6;
    for (const tilt of [-0.35, 0.25]) {
      ctx.beginPath();
      ctx.moveTo(hx, hy - 2);
      ctx.lineTo(hx + Math.sin(tilt) * 8, hy - 2 - Math.cos(tilt) * 8);
      ctx.stroke();
    }
    ctx.strokeStyle = hand;
    ctx.lineWidth = 1.4;
    for (const tilt of [-0.35, 0.25]) {
      ctx.beginPath();
      ctx.moveTo(hx, hy - 2);
      ctx.lineTo(hx + Math.sin(tilt) * 8, hy - 2 - Math.cos(tilt) * 8);
      ctx.stroke();
    }
  }
}

function pointyEars(ctx: CanvasRenderingContext2D, x: number, y: number, sp: Species, t: number, tall: boolean) {
  const twitch = Math.sin(t * 0.7) > 0.97 ? Math.sin(t * 40) * 0.08 : 0; // a rare flick
  for (const side of [-1, 1]) {
    const tipX = x + side * (tall ? 23 : 22);
    const tipY = y - (tall ? 47 : 41);
    ctx.save();
    ctx.translate(x + side * 15, y - 20);
    ctx.rotate(side * twitch);
    ctx.translate(-(x + side * 15), -(y - 20));
    ctx.fillStyle = sp.main;
    ctx.strokeStyle = sp.dark;
    ctx.lineWidth = 1.8;
    ctx.beginPath();
    ctx.moveTo(x + side * 5, y - 25);
    ctx.quadraticCurveTo(tipX - side * 6, tipY + 6, tipX, tipY);
    ctx.quadraticCurveTo(x + side * 27, y - 22, x + side * 25, y - 9);
    ctx.closePath();
    ctx.fill();
    ctx.stroke();
    ctx.fillStyle = sp.accent;
    ctx.beginPath();
    ctx.moveTo(x + side * 10, y - 23);
    ctx.quadraticCurveTo(tipX - side * 4, tipY + 10, tipX - side * 1, tipY + 6);
    ctx.quadraticCurveTo(x + side * 22, y - 20, x + side * 21, y - 13);
    ctx.closePath();
    ctx.fill();
    if (tall) {
      // dark ear tips (fox)
      ctx.fillStyle = sp.dark;
      ctx.beginPath();
      ctx.moveTo(tipX, tipY);
      ctx.lineTo(tipX - side * 5, tipY + 9);
      ctx.lineTo(tipX + side * 2, tipY + 8);
      ctx.closePath();
      ctx.fill();
    }
    ctx.restore();
  }
}

function tail(ctx: CanvasRenderingContext2D, x: number, y: number, sp: Species, t: number, wag: number, stage: number) {
  const sway = Math.sin(t * wag) * 0.35;
  const bx = x + 13;
  const by = y + 48;
  ctx.save();
  ctx.translate(bx, by);
  ctx.rotate(sway - 0.2);
  if (sp.kind === "cat") {
    ctx.strokeStyle = sp.dark;
    ctx.lineWidth = 8;
    ctx.lineCap = "round";
    ctx.beginPath();
    ctx.moveTo(0, 0);
    ctx.bezierCurveTo(16, 2, 22, -14, 16, -28);
    ctx.stroke();
    ctx.strokeStyle = sp.main;
    ctx.lineWidth = 5.5;
    ctx.stroke();
  } else {
    // big fluffy fox tail with a white tip
    ctx.fillStyle = sp.main;
    ctx.strokeStyle = sp.dark;
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    ctx.moveTo(-2, 2);
    ctx.bezierCurveTo(14, 10, 34, -6, 30, -34);
    ctx.bezierCurveTo(22, -26, 6, -14, -2, -6);
    ctx.closePath();
    ctx.fill();
    ctx.stroke();
    ctx.fillStyle = sp.light;
    ctx.beginPath();
    ctx.moveTo(30, -34);
    ctx.bezierCurveTo(31, -24, 28, -18, 24, -16);
    ctx.bezierCurveTo(24, -22, 26, -28, 30, -34);
    ctx.fill();
    if (stage >= 1) {
      // spirit flame on the tip
      const flick = 0.75 + Math.sin(t * 9) * 0.25;
      const g = ctx.createRadialGradient(30, -36, 0, 30, -36, 9 * flick);
      g.addColorStop(0, "rgba(220,245,255,0.95)");
      g.addColorStop(0.5, "rgba(120,200,255,0.7)");
      g.addColorStop(1, "rgba(120,200,255,0)");
      ctx.fillStyle = g;
      ctx.beginPath();
      ctx.arc(30, -36, 9 * flick, 0, TAU);
      ctx.fill();
    }
  }
  ctx.restore();
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, r);
}

// ---------------------------------------------------------------- the pets

export function drawChibi(ctx: CanvasRenderingContext2D, f: Frame, speciesId: string, color: string) {
  const sp = SPECIES[speciesId] ?? SPECIES.neko;
  const stage = Math.max(0, Math.min(3, f.appearance.stage));
  const m = bodyMotion(f, true);
  const { x, y } = m;
  const pose = poseFor(f);
  const scale = STAGE_SCALE[stage];

  ctx.save();
  ctx.translate(x, y + 30);
  ctx.rotate(m.rotation);
  ctx.scale(scale, scale * m.squash);
  ctx.translate(-x, -(y + 30));
  if (f.appearance.weak) ctx.globalAlpha = 0.8;

  // glow in the mood colour behind everything
  const glow = ctx.createRadialGradient(x, y + 14, 10, x, y + 14, 64);
  glow.addColorStop(0, rgba(f.style.halo, Math.min(0.7, f.style.haloAlpha * 1.5)));
  glow.addColorStop(1, rgba(f.style.halo, 0));
  ctx.fillStyle = glow;
  ctx.fillRect(x - 66, y - 52, 132, 132);

  if (sp.kind === "slime") drawSlime(ctx, f, x, y, pose, color, stage);
  else drawBodied(ctx, f, x, y, sp, pose, stage);

  if (f.mood === "sick") {
    // a little green around the gills
    ctx.fillStyle = "rgba(140,200,90,0.18)";
    ctx.beginPath();
    ctx.arc(x, y + 2, HEAD_R, 0, TAU);
    ctx.fill();
  }
  if (f.appearance.hat) drawHat(ctx, f.appearance.hat, x, y + HAT_TOP[sp.kind], f.t);
  ctx.restore();

  if (stage >= 2) drawSparkles(ctx, f, x, y + 10, stage);
  drawMoodExtras(ctx, f, x, y, HEAD_R + 2);
  if (f.emote && !f.emote.id.startsWith("idle-")) drawEmoteExtras(ctx, f.emote, x, y);
}

/** Cat, fox, ninja and robot: big head on a small body with arms and feet. */
function drawBodied(ctx: CanvasRenderingContext2D, f: Frame, x: number, y: number, sp: Species, pose: Pose, stage: number) {
  const t = f.t;
  const isRobot = sp.kind === "robot";
  const isNinja = sp.kind === "ninja";
  const bodyFill = sp.main;
  const handFill = isNinja ? sp.light : isRobot ? sp.light : sp.main;

  if (sp.kind === "cat" || sp.kind === "fox") tail(ctx, x, y, sp, t, pose.wag, stage);
  if (isNinja) scarfTail(ctx, x, y, sp, t);

  // feet
  ctx.fillStyle = isRobot ? sp.dark : isNinja ? sp.dark : sp.main;
  ctx.strokeStyle = sp.dark;
  ctx.lineWidth = 1.5;
  for (const side of [-1, 1]) {
    ctx.beginPath();
    ctx.ellipse(x + side * 8, y + 56, 7, 4.5, 0, 0, TAU);
    ctx.fill();
    ctx.stroke();
  }

  // body
  ctx.fillStyle = bodyFill;
  ctx.strokeStyle = sp.dark;
  ctx.lineWidth = 1.8;
  if (isRobot) {
    roundRect(ctx, x - 15, y + 25, 30, 29, 9);
    ctx.fill();
    ctx.stroke();
    const pulse = 0.55 + Math.sin(t * 3) * 0.35;
    ctx.fillStyle = `rgba(85,208,232,${stage >= 1 ? pulse : 0.6})`;
    ctx.beginPath();
    ctx.arc(x, y + 39, stage >= 1 ? 4.5 : 3.5, 0, TAU);
    ctx.fill();
  } else {
    ctx.beginPath();
    ctx.ellipse(x, y + 40, 16.5, 16, 0, 0, TAU);
    ctx.fill();
    ctx.stroke();
    if (!isNinja) {
      ctx.fillStyle = sp.light;
      ctx.beginPath();
      ctx.ellipse(x, y + 43, 9.5, 10.5, 0, 0, TAU);
      ctx.fill();
    } else {
      ctx.fillStyle = sp.dark; // belt
      ctx.fillRect(x - 15, y + 44, 30, 4);
    }
  }

  // arms hanging down are behind the head; raised arms are drawn after it
  const shoulderY = y + 31;
  const lowered = (a: number) => a < 1.6;
  const drawArm = (side: number, a: number) =>
    arm(ctx, x + side * 13, shoulderY, side, a, isRobot ? sp.dark : bodyFill, handFill, sp.dark, side === 1 && pose.peace);
  if (lowered(pose.armL)) drawArm(-1, pose.armL);
  if (lowered(pose.armR)) drawArm(1, pose.armR);

  // collar with a tiny golden bell once evolved (cat and fox)
  if (stage >= 1 && (sp.kind === "cat" || sp.kind === "fox")) {
    ctx.fillStyle = "#D8423A";
    roundRect(ctx, x - 13, y + 24, 26, 4.5, 2);
    ctx.fill();
    ctx.fillStyle = "#F2C94C";
    ctx.strokeStyle = "#B7892A";
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.arc(x, y + 30.5, 3.4, 0, TAU);
    ctx.fill();
    ctx.stroke();
  }

  // head
  if (sp.kind === "cat") pointyEars(ctx, x, y, sp, t, false);
  if (sp.kind === "fox") pointyEars(ctx, x, y, sp, t, true);
  if (isRobot) drawRobotHead(ctx, f, x, y, sp, stage);
  else {
    ctx.fillStyle = isNinja ? sp.light : sp.main;
    ctx.strokeStyle = sp.dark;
    ctx.lineWidth = 1.8;
    ctx.beginPath();
    ctx.arc(x, y, HEAD_R, 0, TAU);
    ctx.fill();
    ctx.stroke();
    if (sp.kind === "fox") {
      // white cheeks and muzzle
      ctx.fillStyle = sp.light;
      ctx.beginPath();
      ctx.ellipse(x, y + 12, 19, 12, 0, 0, TAU);
      ctx.fill();
    }
    if (sp.kind === "cat") {
      // three little forehead stripes and whiskers
      ctx.strokeStyle = sp.dark;
      ctx.lineWidth = 1.6;
      ctx.lineCap = "round";
      for (const dx of [-5, 0, 5]) {
        ctx.beginPath();
        ctx.moveTo(x + dx, y - 25);
        ctx.lineTo(x + dx * 0.8, y - 19 + Math.abs(dx) * 0.3);
        ctx.stroke();
      }
      ctx.lineWidth = 1;
      for (const side of [-1, 1]) {
        for (const dy of [-1.5, 2]) {
          ctx.beginPath();
          ctx.moveTo(x + side * 20, y + 10 + dy * 0.6);
          ctx.lineTo(x + side * 30, y + 9 + dy * 1.6);
          ctx.stroke();
        }
      }
    }
    animeFace(ctx, x, y + 3, f, sp.iris, sp.kind === "cat" || sp.kind === "fox");
    if (isNinja) drawNinjaHair(ctx, x, y, sp, t, stage);
  }

  if (!lowered(pose.armL)) drawArm(-1, pose.armL);
  if (!lowered(pose.armR)) drawArm(1, pose.armR);
}

function drawNinjaHair(ctx: CanvasRenderingContext2D, x: number, y: number, sp: Species, t: number, stage: number) {
  // spiky dark hair over the top of the head (with a lighter rim so it shows on dark screens)
  ctx.fillStyle = "#2B3152";
  ctx.strokeStyle = "#6672A8";
  ctx.lineWidth = 1.4;
  ctx.lineJoin = "round";
  ctx.beginPath();
  ctx.moveTo(x - 28, y - 2);
  const spikes: [number, number][] = [
    [-31, -18],
    [-24, -26],
    [-26, -38],
    [-12, -33],
    [-6, -45],
    [3, -34],
    [14, -42],
    [18, -30],
    [30, -33],
    [27, -18],
    [32, -10],
  ];
  for (const [dx, dy] of spikes) ctx.lineTo(x + dx, y + dy);
  ctx.lineTo(x + 28, y - 2);
  // bangs over the forehead
  ctx.lineTo(x + 22, y - 12);
  ctx.lineTo(x + 14, y - 6);
  ctx.lineTo(x + 8, y - 13);
  ctx.lineTo(x, y - 6);
  ctx.lineTo(x - 8, y - 13);
  ctx.lineTo(x - 14, y - 6);
  ctx.lineTo(x - 22, y - 12);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();
  // a shine streak in the hair
  ctx.strokeStyle = "rgba(150,165,230,0.6)";
  ctx.lineWidth = 2;
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(x - 14, y - 27);
  ctx.quadraticCurveTo(x - 6, y - 33, x + 4, y - 31);
  ctx.stroke();
  // a plain cloth headband, knotted at the side, with Glowby's own little
  // stitched star in front (an original design: no metal plate, no symbol from any show)
  ctx.fillStyle = sp.accent;
  roundRect(ctx, x - 27, y - 20, 54, 6.5, 3);
  ctx.fill();
  ctx.fillStyle = "rgba(0,0,0,0.18)"; // a fold line
  ctx.fillRect(x - 26, y - 15.5, 52, 1);
  ctx.fillStyle = "#FFF1E6";
  ctx.beginPath();
  ctx.moveTo(x, y - 19.5);
  ctx.quadraticCurveTo(x, y - 16.8, x + 2.8, y - 16.8);
  ctx.quadraticCurveTo(x, y - 16.8, x, y - 14);
  ctx.quadraticCurveTo(x, y - 16.8, x - 2.8, y - 16.8);
  ctx.quadraticCurveTo(x, y - 16.8, x, y - 19.5);
  ctx.fill();
  // the knot
  ctx.fillStyle = sp.accent;
  ctx.beginPath();
  ctx.ellipse(x + 26, y - 17, 3.6, 3, 0, 0, TAU);
  ctx.fill();
  // headband tails flutter behind
  const flap = Math.sin(t * 6);
  const len = stage >= 1 ? 1.25 : 1;
  ctx.strokeStyle = sp.accent;
  ctx.lineWidth = 3.5;
  ctx.lineCap = "round";
  for (const k of [0, 1]) {
    ctx.beginPath();
    ctx.moveTo(x + 26, y - 17);
    ctx.quadraticCurveTo(x + 34 * len, y - 20 + k * 6 + flap * 2, x + (40 + k * 3) * len, y - 12 + k * 7 + flap * 4);
    ctx.stroke();
  }
}

function scarfTail(ctx: CanvasRenderingContext2D, x: number, y: number, sp: Species, t: number) {
  const flap = Math.sin(t * 5);
  ctx.strokeStyle = sp.accent;
  ctx.lineWidth = 5;
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(x - 6, y + 27);
  ctx.quadraticCurveTo(x - 20, y + 30 + flap * 2, x - 28, y + 24 + flap * 5);
  ctx.stroke();
  ctx.fillStyle = sp.accent;
  ctx.beginPath();
  ctx.ellipse(x, y + 26, 14, 4.5, 0, 0, TAU);
  ctx.fill();
}

function drawRobotHead(ctx: CanvasRenderingContext2D, f: Frame, x: number, y: number, sp: Species, stage: number) {
  // antenna with a blinking light
  ctx.strokeStyle = sp.dark;
  ctx.lineWidth = 2.2;
  ctx.beginPath();
  ctx.moveTo(x, y - 24);
  ctx.lineTo(x, y - 35);
  ctx.stroke();
  const blink = Math.sin(f.t * 3) > 0 ? 1 : 0.45;
  ctx.fillStyle = `rgba(85,208,232,${blink})`;
  ctx.beginPath();
  ctx.arc(x, y - 37, stage >= 2 ? 4.2 : 3.5, 0, TAU);
  ctx.fill();
  // ear bolts
  ctx.fillStyle = sp.dark;
  for (const side of [-1, 1]) {
    ctx.beginPath();
    ctx.arc(x + side * 27, y + 2, 4.5, 0, TAU);
    ctx.fill();
  }
  // plated head
  ctx.fillStyle = sp.main;
  ctx.strokeStyle = sp.dark;
  ctx.lineWidth = 1.8;
  roundRect(ctx, x - 26, y - 24, 52, 48, 15);
  ctx.fill();
  ctx.stroke();
  // shine
  ctx.fillStyle = "rgba(255,255,255,0.7)";
  ctx.beginPath();
  ctx.ellipse(x - 13, y - 16, 7, 3, -0.3, 0, TAU);
  ctx.fill();
  // visor
  ctx.fillStyle = "#1E2433";
  roundRect(ctx, x - 20, y - 8, 40, 22, 9);
  ctx.fill();
  ledFace(ctx, x, y + 3, f, sp.accent);
}

/** A cute blob in your chosen colour; little nubs instead of arms. */
function drawSlime(ctx: CanvasRenderingContext2D, f: Frame, x: number, y: number, pose: Pose, color: string, stage: number) {
  const c = COLORWAYS[color] ?? COLORWAYS.periwinkle;
  const wobble = Math.sin(f.t * 3) * 1.5;
  const w = 33 + wobble;
  const top = -25 - wobble;
  const bottom = 58;
  const body = () => {
    ctx.beginPath();
    ctx.moveTo(x - w, y + bottom - 7);
    ctx.bezierCurveTo(x - w - 5, y + 8, x - w * 0.62, y + top, x, y + top);
    ctx.bezierCurveTo(x + w * 0.62, y + top, x + w + 5, y + 8, x + w, y + bottom - 7);
    ctx.quadraticCurveTo(x + w, y + bottom, x + w - 9, y + bottom);
    ctx.lineTo(x - w + 9, y + bottom);
    ctx.quadraticCurveTo(x - w, y + bottom, x - w, y + bottom - 7);
    ctx.closePath();
  };
  // nubs (behind when low, in front when raised)
  const nub = (side: number, a: number) => {
    const lift = Math.min(1, a / 2.6);
    ctx.fillStyle = rgba(c.bell);
    ctx.strokeStyle = rgba(c.edge);
    ctx.lineWidth = 1.6;
    ctx.beginPath();
    ctx.ellipse(x + side * (w + 2), y + 34 - lift * 22, 6.5, 5.5, side * lift, 0, TAU);
    ctx.fill();
    ctx.stroke();
  };
  nub(-1, pose.armL);
  nub(1, pose.armR);
  const g = ctx.createRadialGradient(x - 10, y + 2, 4, x, y + 16, 52);
  g.addColorStop(0, rgba(c.inner));
  g.addColorStop(0.55, rgba(c.bell));
  g.addColorStop(1, rgba(c.edge, 0.95));
  ctx.fillStyle = g;
  ctx.strokeStyle = rgba(c.edge);
  ctx.lineWidth = 2;
  body();
  ctx.fill();
  ctx.stroke();
  // glossy highlights
  ctx.fillStyle = "rgba(255,255,255,0.75)";
  ctx.beginPath();
  ctx.ellipse(x - 15, y - 8, 7.5, 4, -0.6, 0, TAU);
  ctx.fill();
  ctx.beginPath();
  ctx.arc(x - 6, y - 15, 2, 0, TAU);
  ctx.fill();
  if (stage >= 1) {
    // a little glowing core
    const core = ctx.createRadialGradient(x, y + 34, 0, x, y + 34, 9);
    core.addColorStop(0, "rgba(255,248,210,0.9)");
    core.addColorStop(1, "rgba(255,248,210,0)");
    ctx.fillStyle = core;
    ctx.beginPath();
    ctx.arc(x, y + 34, 9, 0, TAU);
    ctx.fill();
  }
  animeFace(ctx, x, y + 14, f, "#3A3F66", false);
}
