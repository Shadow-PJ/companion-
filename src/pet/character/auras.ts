// Power-up auras, unlocked with levels. Drawn around Glowby or your imported
// character, in code like everything else. Each one is a generic effect
// (flames, lightning, petals, rings), not copied from any show.
//
// Most of an aura is drawn BEHIND the body; a few bits (petals, sparks) are
// drawn in FRONT so they float over it.

const TAU = Math.PI * 2;

export const AURAS = ["sparkle", "flame", "sakura", "lightning", "cursed", "infinity", "sun", "rainbow"] as const;

/** A repeatable pseudo-random number in 0..1 for a seed (no Math.random, so it never jitters). */
function noise(seed: number) {
  const s = Math.sin(seed * 127.1 + 311.7) * 43758.5453;
  return s - Math.floor(s);
}

/** Soft round glow. `rgb` like "255,140,40". */
function glow(ctx: CanvasRenderingContext2D, x: number, y: number, inner: number, outer: number, rgb: string, alpha: number) {
  const g = ctx.createRadialGradient(x, y, inner, x, y, outer);
  g.addColorStop(0, `rgba(${rgb},${alpha})`);
  g.addColorStop(1, `rgba(${rgb},0)`);
  ctx.fillStyle = g;
  ctx.fillRect(x - outer, y - outer, outer * 2, outer * 2);
}

function star4(ctx: CanvasRenderingContext2D, x: number, y: number, r: number) {
  ctx.beginPath();
  ctx.moveTo(x, y - r);
  ctx.quadraticCurveTo(x, y, x + r, y);
  ctx.quadraticCurveTo(x, y, x, y + r);
  ctx.quadraticCurveTo(x, y, x - r, y);
  ctx.quadraticCurveTo(x, y, x, y - r);
  ctx.fill();
}

/** One flame tongue from (bx, by) pointing along (dx, dy). */
function tongue(ctx: CanvasRenderingContext2D, bx: number, by: number, dx: number, dy: number, len: number, width: number, inner: string, outer: string) {
  const tx = bx + dx * len;
  const ty = by + dy * len;
  const nx = -dy * width;
  const ny = dx * width;
  const g = ctx.createLinearGradient(bx, by, tx, ty);
  g.addColorStop(0, inner);
  g.addColorStop(1, outer);
  ctx.fillStyle = g;
  ctx.beginPath();
  ctx.moveTo(bx + nx, by + ny);
  ctx.quadraticCurveTo(bx + nx + dx * len * 0.5, by + ny + dy * len * 0.5, tx, ty);
  ctx.quadraticCurveTo(bx - nx + dx * len * 0.5, by - ny + dy * len * 0.5, bx - nx, by - ny);
  ctx.closePath();
  ctx.fill();
}

/**
 * Draws one layer of an aura around a body centred at (x, y) with radius r.
 * `power` 0..1 dims it (sleepy or tired Glowby has a weaker aura).
 */
export function drawAura(
  ctx: CanvasRenderingContext2D,
  id: string,
  x: number,
  y: number,
  r: number,
  t: number,
  layer: "back" | "front",
  power = 1,
) {
  if (!id || power <= 0) return;
  ctx.save();
  ctx.globalAlpha *= power;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  if (layer === "back") back(ctx, id, x, y, r, t);
  else front(ctx, id, x, y, r, t);
  ctx.restore();
}

function back(ctx: CanvasRenderingContext2D, id: string, x: number, y: number, r: number, t: number) {
  switch (id) {
    case "sparkle": {
      glow(ctx, x, y, r * 0.6, r * 1.55, "190,205,255", 0.4);
      for (let i = 0; i < 8; i++) {
        const a = t * 0.6 + (i * TAU) / 8;
        const rr = r + 11 + Math.sin(t * 2 + i) * 3;
        ctx.fillStyle = `rgba(255,255,255,${0.45 + 0.55 * Math.abs(Math.sin(t * 2.5 + i))})`;
        star4(ctx, x + Math.cos(a) * rr, y + Math.sin(a) * rr * 0.92, 2.5 + 1.8 * Math.abs(Math.sin(t * 3 + i * 1.7)));
      }
      break;
    }
    case "flame": {
      glow(ctx, x, y, r * 0.7, r * 1.65, "255,130,40", 0.42);
      const n = 18;
      for (const [scale, inner, outer] of [
        [1, "rgba(255,120,30,0.85)", "rgba(220,40,10,0)"],
        [0.62, "rgba(255,225,110,0.95)", "rgba(255,150,40,0)"],
      ] as const) {
        for (let i = 0; i < n; i++) {
          const a = (i / n) * TAU + 0.17;
          const up = (1 - Math.sin(a)) / 2; // 1 at the top, 0 at the bottom: flames rise
          const flicker = 0.65 + 0.35 * Math.sin(t * 9 + i * 2.3) * Math.sin(t * 5.3 + i);
          const ox = Math.cos(a);
          const oy = Math.sin(a);
          let dx = ox;
          let dy = oy - 0.9 * up - 0.4;
          const d = Math.hypot(dx, dy) || 1;
          dx /= d;
          dy /= d;
          const len = (7 + 20 * up) * flicker * scale;
          tongue(ctx, x + ox * (r - 3), y + oy * (r - 3), dx, dy, len + 6, 5.5 * scale, inner, outer);
        }
      }
      break;
    }
    case "sakura": {
      glow(ctx, x, y, r * 0.6, r * 1.5, "255,183,207", 0.35);
      break;
    }
    case "lightning": {
      glow(ctx, x, y, r * 0.6, r * 1.75, "255,214,80", 0.45);
      // a spiky golden flare that flickers, taller at the top
      const spikes = 20;
      ctx.beginPath();
      for (let i = 0; i < spikes * 2; i++) {
        const a = (i / (spikes * 2)) * TAU - Math.PI / 2;
        const up = (1 - Math.sin(a)) / 2;
        const rr = i % 2 ? r + 3 : r + 8 + 18 * up * (0.7 + 0.3 * Math.sin(t * 14 + i * 1.3));
        const px = x + Math.cos(a) * rr;
        const py = y + Math.sin(a) * rr - up * 5;
        if (i === 0) ctx.moveTo(px, py);
        else ctx.lineTo(px, py);
      }
      ctx.closePath();
      const g = ctx.createRadialGradient(x, y - 4, r - 2, x, y - 4, r + 30);
      g.addColorStop(0, "rgba(255,240,150,0.9)");
      g.addColorStop(0.45, "rgba(255,205,60,0.6)");
      g.addColorStop(1, "rgba(255,180,30,0)");
      ctx.fillStyle = g;
      ctx.fill();
      break;
    }
    case "cursed": {
      glow(ctx, x, y, r * 0.5, r * 1.7, "60,10,90", 0.6);
      for (let i = 0; i < 10; i++) {
        const a = (i / 10) * TAU + t * 0.5;
        const wob = Math.sin(t * 3 + i) * 4;
        const p = (rr: number, da: number) => [x + Math.cos(a + da) * rr, y + Math.sin(a + da) * rr] as const;
        const [sx, sy] = p(r + 1, 0);
        const [c1x, c1y] = p(r + 14 + wob, 0.45);
        const [ex, ey] = p(r + 24 + wob, 0.95);
        ctx.strokeStyle = i % 2 ? "rgba(130,50,210,0.8)" : "rgba(25,0,40,0.85)";
        ctx.lineWidth = i % 2 ? 2.4 : 3.2;
        ctx.beginPath();
        ctx.moveTo(sx, sy);
        ctx.quadraticCurveTo(c1x, c1y, ex, ey);
        ctx.stroke();
      }
      break;
    }
    case "infinity": {
      glow(ctx, x, y, r * 0.6, r * 1.6, "120,190,255", 0.4);
      const rings: [number, number, number, number][] = [
        [6, 2.2, 0.85, 1.4],
        [13, 1.6, 0.6, -0.9],
        [20, 1.1, 0.4, 0.6],
      ];
      for (const [gap, width, alpha, speed] of rings) {
        ctx.strokeStyle = `rgba(160,215,255,${alpha})`;
        ctx.lineWidth = width;
        ctx.setLineDash([16 + gap, 7]);
        ctx.lineDashOffset = t * speed * 20;
        ctx.beginPath();
        ctx.arc(x, y, r + gap, 0, TAU);
        ctx.stroke();
      }
      ctx.setLineDash([]);
      break;
    }
    case "sun": {
      glow(ctx, x, y, r * 0.7, r * 1.95, "255,190,60", 0.5);
      const rays = 14;
      for (let i = 0; i < rays; i++) {
        const a = t * 0.25 + (i * TAU) / rays;
        const len = r + 14 + (i % 2 ? 7 : 0) + Math.sin(t * 3 + i) * 2.5;
        const w = 0.1;
        const g = ctx.createLinearGradient(x, y, x + Math.cos(a) * len, y + Math.sin(a) * len);
        g.addColorStop(0.5, "rgba(255,210,90,0.85)");
        g.addColorStop(1, "rgba(255,150,40,0)");
        ctx.fillStyle = g;
        ctx.beginPath();
        ctx.moveTo(x + Math.cos(a - w) * (r - 2), y + Math.sin(a - w) * (r - 2));
        ctx.lineTo(x + Math.cos(a) * len, y + Math.sin(a) * len);
        ctx.lineTo(x + Math.cos(a + w) * (r - 2), y + Math.sin(a + w) * (r - 2));
        ctx.closePath();
        ctx.fill();
      }
      break;
    }
    case "rainbow": {
      glow(ctx, x, y, r * 0.7, r * 1.6, "255,255,255", 0.18);
      const g = ctx.createConicGradient(t * 0.5, x, y);
      const hues = [0, 45, 90, 160, 210, 270, 320, 360];
      hues.forEach((h, i) => g.addColorStop(i / (hues.length - 1), `hsla(${h},90%,65%,0.6)`));
      ctx.strokeStyle = g;
      ctx.lineWidth = 7;
      ctx.beginPath();
      ctx.arc(x, y, r + 10, 0, TAU);
      ctx.stroke();
      ctx.lineWidth = 2;
      ctx.globalAlpha *= 0.5;
      ctx.beginPath();
      ctx.arc(x, y, r + 18 + Math.sin(t * 2) * 2, 0, TAU);
      ctx.stroke();
      break;
    }
  }
}

function front(ctx: CanvasRenderingContext2D, id: string, x: number, y: number, r: number, t: number) {
  switch (id) {
    case "sakura": {
      // petals drifting down past the body
      for (let i = 0; i < 9; i++) {
        const p = (t * 0.16 + noise(i)) % 1;
        const px = x + (noise(i * 3.1) - 0.5) * 2 * (r + 22) + Math.sin(t * 1.5 + i) * 7;
        const py = y - r - 12 + p * (2 * r + 34);
        ctx.save();
        ctx.translate(px, py);
        ctx.rotate(t * 1.8 + i * 1.3);
        ctx.globalAlpha *= Math.sin(p * Math.PI);
        ctx.fillStyle = i % 3 ? "#FFB7CF" : "#FFD9E6";
        ctx.beginPath();
        ctx.moveTo(0, -4.5);
        ctx.quadraticCurveTo(3.6, -1, 0, 4.5);
        ctx.quadraticCurveTo(-3.6, -1, 0, -4.5);
        ctx.fill();
        ctx.restore();
      }
      break;
    }
    case "lightning": {
      // crackling bolts, re-drawn ~8 times a second
      const frame = Math.floor(t * 8);
      for (let b = 0; b < 2; b++) {
        if (noise(frame * 3 + b) < 0.45) continue;
        const a0 = noise(frame * 7 + b * 13) * TAU;
        const pts: [number, number][] = [];
        for (let k = 0; k <= 4; k++) {
          const rr = r + 2 + k * 6;
          const a = a0 + (noise(frame * 11 + b * 5 + k) - 0.5) * 0.5;
          pts.push([x + Math.cos(a) * rr, y + Math.sin(a) * rr]);
        }
        for (const [width, color] of [
          [3.5, "rgba(255,230,120,0.45)"],
          [1.4, "rgba(255,255,225,0.95)"],
        ] as const) {
          ctx.strokeStyle = color;
          ctx.lineWidth = width;
          ctx.beginPath();
          pts.forEach(([px, py], k) => (k ? ctx.lineTo(px, py) : ctx.moveTo(px, py)));
          ctx.stroke();
        }
      }
      break;
    }
    case "cursed": {
      // red sparks orbiting
      for (let i = 0; i < 5; i++) {
        const a = t * (0.8 + noise(i) * 0.6) + i * 1.9;
        const rr = r + 6 + noise(i * 5) * 18;
        ctx.fillStyle = `rgba(235,45,70,${0.5 + 0.5 * Math.abs(Math.sin(t * 4 + i))})`;
        ctx.beginPath();
        ctx.arc(x + Math.cos(a) * rr, y + Math.sin(a) * rr, 1.6, 0, TAU);
        ctx.fill();
      }
      break;
    }
    case "infinity": {
      // tiny lights pulled in toward the centre
      for (let i = 0; i < 8; i++) {
        const p = (t * 0.45 + i / 8) % 1;
        const a = i * 2.4 + t * 0.3;
        const rr = r + 30 - p * 26;
        ctx.fillStyle = `rgba(200,235,255,${p * 0.9})`;
        ctx.beginPath();
        ctx.arc(x + Math.cos(a) * rr, y + Math.sin(a) * rr, 1.4, 0, TAU);
        ctx.fill();
      }
      break;
    }
  }
}
