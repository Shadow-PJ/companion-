// An imported character, drawn as a round "sticker" icon with all of Glowby's
// body language: bobbing, moods, hats, emotes and evolution rings.

import { bodyMotion, drawEmoteExtras, drawMoodExtras, drawSparkles, type Frame, STAGE_SCALE } from "./glowby";
import { drawHat } from "./hats";
import { hueShift, rgba } from "./palette";

/** Radius of the round icon (CSS pixels, before evolution growth). */
export const AVATAR_R = 42;
const TAU = Math.PI * 2;

export function drawAvatar(ctx: CanvasRenderingContext2D, f: Frame, img: ImageBitmap | null) {
  const s = f.style;
  const stage = Math.max(0, Math.min(3, f.appearance.stage));
  const weak = f.appearance.weak;
  const m = bodyMotion(f);
  const { x, y } = m;
  const r = AVATAR_R * (1 + m.pulse * 0.02 * m.pulseAmp); // a gentle "breath"
  const scale = STAGE_SCALE[stage];

  ctx.save();
  ctx.translate(x, y);
  ctx.rotate(m.rotation);
  ctx.scale(scale, scale * m.squash);
  ctx.translate(-x, -y);
  if (weak) ctx.globalAlpha = 0.8;

  // Glow in the mood colour: blue while working, amber when Claude needs you, green when sick.
  const outer = r + 26;
  const glow = ctx.createRadialGradient(x, y, r * 0.6, x, y, outer);
  glow.addColorStop(0, rgba(s.halo, Math.min(0.75, s.haloAlpha * 1.8)));
  glow.addColorStop(1, rgba(s.halo, 0));
  ctx.fillStyle = glow;
  ctx.fillRect(x - outer, y - outer, outer * 2, outer * 2);

  // Dark rim first, so the icon stands out on any wallpaper.
  ctx.fillStyle = rgba(s.edge);
  ctx.beginPath();
  ctx.arc(x, y, r + 3.5, 0, TAU);
  ctx.fill();

  // The picture, clipped to a circle. It leans a little toward the mouse.
  ctx.save();
  ctx.beginPath();
  ctx.arc(x, y, r, 0, TAU);
  ctx.clip();
  ctx.fillStyle = rgba(s.inner);
  ctx.fillRect(x - r, y - r, r * 2, r * 2);
  if (img) {
    const lean = 3;
    const filters: string[] = [];
    if (f.mood === "sleepy") filters.push("saturate(0.55)", "brightness(0.92)");
    if (weak) filters.push("saturate(0.5)");
    if (filters.length) ctx.filter = filters.join(" ");
    ctx.drawImage(img, x - r - lean + f.look.x * lean, y - r - lean + f.look.y * lean, (r + lean) * 2, (r + lean) * 2);
    ctx.filter = "none";
  } else {
    ctx.fillStyle = rgba(s.edge, 0.55);
    ctx.font = "600 30px 'Segoe UI', sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText("?", x, y + 1);
  }
  if (f.mood === "sick") {
    ctx.fillStyle = "rgba(150,210,100,0.3)";
    ctx.fillRect(x - r, y - r, r * 2, r * 2);
  } else if (f.mood === "alert") {
    ctx.fillStyle = `rgba(255,190,80,${0.1 + 0.1 * Math.sin(f.t * 8)})`;
    ctx.fillRect(x - r, y - r, r * 2, r * 2);
  }
  // glossy highlight, like a sticker
  ctx.fillStyle = "rgba(255,255,255,0.2)";
  ctx.beginPath();
  ctx.ellipse(x - r * 0.28, y - r * 0.58, r * 0.5, r * 0.24, -0.35, 0, TAU);
  ctx.fill();
  ctx.restore();

  // Ring in the mood colour. Evolving adds a gold ring, then studs; Aurora cycles colours.
  ctx.lineWidth = 3;
  if (stage >= 3) {
    const g = ctx.createConicGradient(f.t * 0.6, x, y);
    for (let i = 0; i <= 6; i++) g.addColorStop(i / 6, rgba(hueShift(s.bell, i * 60)));
    ctx.strokeStyle = g;
  } else {
    ctx.strokeStyle = rgba(s.bell);
  }
  ctx.beginPath();
  ctx.arc(x, y, r + 1.5, 0, TAU);
  ctx.stroke();
  if (stage >= 1) {
    ctx.strokeStyle = "rgba(255,226,140,0.9)";
    ctx.lineWidth = 1.3;
    ctx.beginPath();
    ctx.arc(x, y, r + 4.8, 0, TAU);
    ctx.stroke();
  }
  if (stage >= 2) {
    for (let i = 0; i < 8; i++) {
      const a = f.t * 0.3 + (i * TAU) / 8;
      ctx.fillStyle = "rgba(255,244,200,0.95)";
      ctx.beginPath();
      ctx.arc(x + Math.cos(a) * (r + 4.8), y + Math.sin(a) * (r + 4.8), 1.8, 0, TAU);
      ctx.fill();
    }
  }

  // Working: a spinning "busy" ring around the icon.
  if (f.mood === "working") {
    ctx.strokeStyle = rgba(s.edge, 0.9);
    ctx.lineWidth = 2.4;
    ctx.lineCap = "round";
    const a = f.t * 4;
    for (const offset of [0, Math.PI]) {
      ctx.beginPath();
      ctx.arc(x, y, r + 9, a + offset, a + offset + 1.1);
      ctx.stroke();
    }
  }

  if (f.appearance.hat) drawHat(ctx, f.appearance.hat, x, y - r - 1, f.t);
  ctx.restore();

  if (stage >= 2) drawSparkles(ctx, f, x, y, stage);
  drawMoodExtras(ctx, f, x, y, r + 4);
  if (f.emote) drawEmoteExtras(ctx, f.emote, x, y);
}
