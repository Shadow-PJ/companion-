// The crop tool for imported characters: drag to move, scroll (or the slider)
// to zoom, and the part inside the circle becomes Glowby's 256×256 icon.
// The browser engine decodes the picture; nothing leaves this PC.

import { button, el } from "../shared/dom";

const VIEW = 260; // crop area, CSS pixels
const CIRCLE = 220; // diameter of the circle inside it
const OUT = 256; // saved icon size
const MAX_ZOOM = 6;

export interface CropResult {
  name: string;
  png: Blob;
}

/** "gojo_satoru-01.png" → "Gojo satoru 01" */
export function nameFromFile(path: string): string {
  const base = path.split(/[\\/]/).pop() ?? "";
  const words = base.replace(/\.[^.]+$/, "").replace(/[_\-.]+/g, " ").trim();
  return words ? words[0].toUpperCase() + words.slice(1) : "";
}

/**
 * Shows the crop tool inside `host` for `bitmap`. Resolves with the result,
 * or null if you cancel.
 */
export function crop(host: HTMLElement, bitmap: ImageBitmap, suggestedName: string): Promise<CropResult | null> {
  return new Promise((resolve) => {
    const dpr = Math.min(window.devicePixelRatio || 1, 3);
    const canvas = el("canvas", { class: "crop-canvas", width: Math.round(VIEW * dpr), height: Math.round(VIEW * dpr) });
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const zoomSlider = el("input", { type: "range", min: 100, max: MAX_ZOOM * 100, step: 1, value: 100, class: "range" });
    const name = el("input", { type: "text", class: "text", maxlength: 40, placeholder: "Character name", value: suggestedName });
    const saveBtn = button("Save character", "primary", () => void finish());
    const cancelBtn = button("Cancel", "", () => done(null));

    const w = bitmap.width;
    const h = bitmap.height;
    const cover = CIRCLE / Math.min(w, h); // smallest scale that fills the circle
    let zoom = 1;
    let cx = w / 2; // image point shown at the circle's centre
    let cy = h / 2;
    const scale = () => cover * zoom;

    function clampCentre() {
      const half = CIRCLE / 2 / scale();
      cx = Math.min(Math.max(cx, half), w - half);
      cy = Math.min(Math.max(cy, half), h - half);
    }

    function draw() {
      const s = scale();
      ctx.clearRect(0, 0, VIEW, VIEW);
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(bitmap, VIEW / 2 - cx * s, VIEW / 2 - cy * s, w * s, h * s);
      // darken everything outside the circle
      ctx.fillStyle = "rgba(10,12,30,0.55)";
      ctx.beginPath();
      ctx.rect(0, 0, VIEW, VIEW);
      ctx.arc(VIEW / 2, VIEW / 2, CIRCLE / 2, 0, Math.PI * 2);
      ctx.fill("evenodd");
      ctx.strokeStyle = "rgba(255,255,255,0.9)";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.arc(VIEW / 2, VIEW / 2, CIRCLE / 2, 0, Math.PI * 2);
      ctx.stroke();
    }

    function setZoom(z: number) {
      zoom = Math.min(MAX_ZOOM, Math.max(1, z));
      zoomSlider.value = String(Math.round(zoom * 100));
      clampCentre();
      draw();
    }

    let drag: { x: number; y: number } | null = null;
    canvas.addEventListener("pointerdown", (e) => {
      drag = { x: e.clientX, y: e.clientY };
      canvas.setPointerCapture(e.pointerId);
    });
    canvas.addEventListener("pointermove", (e) => {
      if (!drag) return;
      cx -= (e.clientX - drag.x) / scale();
      cy -= (e.clientY - drag.y) / scale();
      drag = { x: e.clientX, y: e.clientY };
      clampCentre();
      draw();
    });
    canvas.addEventListener("pointerup", () => (drag = null));
    canvas.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        setZoom(zoom * Math.exp(-e.deltaY * 0.0015));
      },
      { passive: false },
    );
    zoomSlider.addEventListener("input", () => setZoom(Number(zoomSlider.value) / 100));

    async function finish() {
      saveBtn.disabled = true;
      const out = document.createElement("canvas");
      out.width = OUT;
      out.height = OUT;
      const octx = out.getContext("2d")!;
      octx.imageSmoothingQuality = "high";
      const s = (scale() * OUT) / CIRCLE;
      octx.drawImage(bitmap, OUT / 2 - cx * s, OUT / 2 - cy * s, w * s, h * s);
      const png = await new Promise<Blob | null>((r) => out.toBlob(r, "image/png"));
      if (!png) {
        saveBtn.disabled = false;
        return;
      }
      done({ name: name.value.trim(), png });
    }

    function done(result: CropResult | null) {
      host.hidden = true;
      host.replaceChildren();
      resolve(result);
    }

    host.hidden = false;
    host.replaceChildren(
      el("h3", { text: "Crop your character" }),
      el("p", { class: "hint", text: "Drag to move, scroll to zoom. What's inside the circle becomes the icon. (Animated GIFs use their first frame.)" }),
      el(
        "div",
        { class: "crop" },
        canvas,
        el(
          "div",
          { class: "crop-side" },
          el("label", { class: "hint", text: "Zoom" }),
          zoomSlider,
          el("label", { class: "hint", text: "Name" }),
          name,
          el("div", { class: "actions" }, saveBtn, cancelBtn),
        ),
      ),
    );
    draw();
    name.focus();
  });
}
