// Unlockable hats, drawn in code. (x, top) is the top centre of Glowby's bell.

const TAU = Math.PI * 2;

function circle(ctx: CanvasRenderingContext2D, x: number, y: number, r: number, color: string) {
  ctx.fillStyle = color;
  ctx.beginPath();
  ctx.arc(x, y, r, 0, TAU);
  ctx.fill();
}

function star(ctx: CanvasRenderingContext2D, x: number, y: number, r: number, color: string) {
  ctx.fillStyle = color;
  ctx.beginPath();
  for (let i = 0; i < 10; i++) {
    const a = -Math.PI / 2 + (i * Math.PI) / 5;
    const rr = i % 2 ? r * 0.45 : r;
    ctx.lineTo(x + Math.cos(a) * rr, y + Math.sin(a) * rr);
  }
  ctx.closePath();
  ctx.fill();
}

export function drawHat(ctx: CanvasRenderingContext2D, id: string, x: number, top: number, t: number) {
  ctx.save();
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  switch (id) {
    case "sprout": {
      ctx.strokeStyle = "#3B6D11";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(x, top + 2);
      ctx.quadraticCurveTo(x + 1, top - 6, x, top - 10);
      ctx.stroke();
      const sway = Math.sin(t * 2) * 0.12;
      for (const side of [-1, 1]) {
        ctx.save();
        ctx.translate(x, top - 9);
        ctx.rotate(side * (0.9 + sway));
        ctx.fillStyle = "#6BBF59";
        ctx.beginPath();
        ctx.ellipse(0, -6, 4, 7, 0, 0, TAU);
        ctx.fill();
        ctx.restore();
      }
      break;
    }
    case "party": {
      ctx.save();
      ctx.translate(x + 3, top + 3);
      ctx.rotate(0.18);
      ctx.fillStyle = "#F48FB8";
      ctx.beginPath();
      ctx.moveTo(-10, 0);
      ctx.lineTo(10, 0);
      ctx.lineTo(0, -26);
      ctx.closePath();
      ctx.fill();
      ctx.strokeStyle = "#FAC775";
      ctx.lineWidth = 2.5;
      for (const y of [-7, -15]) {
        const half = 10 * (1 + y / 26);
        ctx.beginPath();
        ctx.moveTo(-half, y);
        ctx.lineTo(half, y);
        ctx.stroke();
      }
      circle(ctx, 0, -27, 3.6, "#FAC775");
      ctx.restore();
      break;
    }
    case "beanie": {
      ctx.fillStyle = "#E2725B";
      ctx.beginPath();
      ctx.ellipse(x, top + 9, 21, 15, 0, Math.PI, TAU);
      ctx.fill();
      ctx.fillStyle = "#C9553F";
      ctx.beginPath();
      ctx.roundRect(x - 23, top + 5, 46, 7, 3.5);
      ctx.fill();
      ctx.strokeStyle = "rgba(255,255,255,0.25)";
      ctx.lineWidth = 1;
      for (let i = -18; i <= 18; i += 4) {
        ctx.beginPath();
        ctx.moveTo(x + i, top + 6);
        ctx.lineTo(x + i, top + 11);
        ctx.stroke();
      }
      circle(ctx, x, top - 7, 5, "#F7E3D2");
      break;
    }
    case "headphones": {
      ctx.strokeStyle = "#2F3346";
      ctx.lineWidth = 3.5;
      ctx.beginPath();
      ctx.arc(x, top + 22, 33, Math.PI * 1.12, Math.PI * 1.88);
      ctx.stroke();
      for (const side of [-1, 1]) {
        ctx.fillStyle = "#2F3346";
        ctx.beginPath();
        ctx.roundRect(x + side * 33 - 5, top + 13, 10, 16, 4);
        ctx.fill();
        ctx.fillStyle = "#7F9BFF";
        ctx.beginPath();
        ctx.roundRect(x + side * 33 - 2.5, top + 16, 5, 10, 2.5);
        ctx.fill();
      }
      break;
    }
    case "crown": {
      ctx.fillStyle = "#F2C14E";
      ctx.strokeStyle = "#B8862B";
      ctx.lineWidth = 1.2;
      ctx.beginPath();
      ctx.moveTo(x - 12, top + 3);
      ctx.lineTo(x - 13, top - 9);
      ctx.lineTo(x - 6, top - 3);
      ctx.lineTo(x, top - 12);
      ctx.lineTo(x + 6, top - 3);
      ctx.lineTo(x + 13, top - 9);
      ctx.lineTo(x + 12, top + 3);
      ctx.closePath();
      ctx.fill();
      ctx.stroke();
      circle(ctx, x, top - 2, 2, "#E24B4A");
      circle(ctx, x - 7, top, 1.5, "#5DCAA5");
      circle(ctx, x + 7, top, 1.5, "#5DCAA5");
      break;
    }
    case "wizard": {
      ctx.fillStyle = "#5B4BB7";
      ctx.beginPath();
      ctx.ellipse(x, top + 4, 22, 5, 0, 0, TAU);
      ctx.fill();
      ctx.beginPath();
      ctx.moveTo(x - 13, top + 3);
      ctx.quadraticCurveTo(x - 4, top - 18, x + 9, top - 32);
      ctx.quadraticCurveTo(x + 6, top - 14, x + 13, top + 3);
      ctx.closePath();
      ctx.fill();
      star(ctx, x - 2, top - 9, 3.5, "#FAC775");
      star(ctx, x + 5, top - 19, 2.5, "#FAC775");
      break;
    }
    case "gradcap": {
      ctx.fillStyle = "#2F3346";
      ctx.beginPath();
      ctx.roundRect(x - 10, top - 3, 20, 8, 2);
      ctx.fill();
      ctx.beginPath();
      ctx.moveTo(x - 22, top - 5);
      ctx.lineTo(x, top - 13);
      ctx.lineTo(x + 22, top - 5);
      ctx.lineTo(x, top + 3);
      ctx.closePath();
      ctx.fill();
      ctx.strokeStyle = "#FAC775";
      ctx.lineWidth = 1.5;
      const swing = Math.sin(t * 1.8) * 2;
      ctx.beginPath();
      ctx.moveTo(x, top - 5);
      ctx.lineTo(x + 15, top - 4);
      ctx.lineTo(x + 16 + swing, top + 8);
      ctx.stroke();
      circle(ctx, x + 16 + swing, top + 9, 2, "#FAC775");
      break;
    }
    case "detective": {
      // a tweed deerstalker: round crown, a brim in front and behind, flaps tied on top
      const tweed = "#8C6B47";
      const dark = "#5C4428";
      ctx.fillStyle = dark;
      ctx.beginPath();
      ctx.ellipse(x - 14, top + 1, 9, 3.4, -0.3, 0, TAU);
      ctx.ellipse(x + 14, top + 1, 9, 3.4, 0.3, 0, TAU);
      ctx.fill();
      ctx.save();
      ctx.beginPath();
      ctx.moveTo(x - 15, top + 2);
      ctx.bezierCurveTo(x - 15, top - 16, x + 15, top - 16, x + 15, top + 2);
      ctx.closePath();
      ctx.fillStyle = tweed;
      ctx.fill();
      ctx.clip();
      // the check pattern
      ctx.strokeStyle = "rgba(255,226,180,0.35)";
      ctx.lineWidth = 1;
      for (const dx of [-9, -3, 3, 9]) {
        ctx.beginPath();
        ctx.moveTo(x + dx, top - 14);
        ctx.lineTo(x + dx, top + 3);
        ctx.stroke();
      }
      for (const dy of [-7, -2]) {
        ctx.beginPath();
        ctx.moveTo(x - 16, top + dy);
        ctx.lineTo(x + 16, top + dy);
        ctx.stroke();
      }
      ctx.restore();
      // the little bow on top
      ctx.fillStyle = dark;
      ctx.beginPath();
      ctx.ellipse(x - 3.2, top - 12.5, 3, 1.7, -0.4, 0, TAU);
      ctx.ellipse(x + 3.2, top - 12.5, 3, 1.7, 0.4, 0, TAU);
      ctx.fill();
      circle(ctx, x, top - 12.5, 1.6, dark);
      break;
    }
  }
  ctx.restore();
}
