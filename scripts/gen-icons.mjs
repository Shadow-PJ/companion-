// Draws Glowby's icon in code (no image files, no image libraries) and writes
// the PNG / ICO files Tauri needs. Run: npm run icons
//
// How it works: for every pixel we test 4x4 sample points against simple
// shapes (circles, a dome, wavy lines) and average them, which gives smooth
// antialiased edges. Then we encode PNG ourselves (zlib is built into Node).

import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "icons");

const COLORS = {
  halo: [133, 160, 255, 0.28],
  edge: [92, 119, 214, 1],
  bell: [175, 194, 255, 1],
  shine: [255, 255, 255, 0.55],
  tentacle: [224, 140, 198, 1],
  ink: [36, 38, 58, 1],
  cheek: [237, 110, 140, 0.45],
};

// Shapes in a 0..1 unit square. Later shapes paint over earlier ones.
function insideBell(x, y, grow = 0) {
  const cx = 0.5, cy = 0.5, rx = 0.36 + grow, ry = 0.34 + grow;
  if (y <= cy) return ((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2 <= 1;
  // skirt with a scalloped bottom edge
  const bottom = cy + 0.07 + grow + 0.025 * Math.cos(((x - cx) / rx) * Math.PI * 3);
  return Math.abs(x - cx) <= rx && y <= bottom;
}

function nearTentacle(x, y) {
  for (const tx of [0.32, 0.44, 0.56, 0.68]) {
    if (y < 0.6 || y > 0.95) continue;
    const wave = tx + 0.035 * Math.sin((y - 0.6) * 18 + tx * 10);
    const width = 0.045 * (1 - (y - 0.6) / 0.5);
    if (Math.abs(x - wave) < width) return true;
  }
  return false;
}

function paint(x, y) {
  const layers = [];
  if (((x - 0.5) ** 2 + (y - 0.52) ** 2) <= 0.47 ** 2) layers.push(COLORS.halo);
  if (nearTentacle(x, y)) layers.push(COLORS.tentacle);
  if (insideBell(x, y, 0.035)) layers.push(COLORS.edge);
  if (insideBell(x, y)) layers.push(COLORS.bell);
  if (((x - 0.38) / 0.07) ** 2 + ((y - 0.3) / 0.04) ** 2 <= 1) layers.push(COLORS.shine);
  for (const ex of [0.4, 0.6]) {
    if (((x - ex) / 0.045) ** 2 + ((y - 0.5) / 0.06) ** 2 <= 1) layers.push(COLORS.ink);
    if (((x - ex - 0.012) / 0.016) ** 2 + ((y - 0.48) / 0.018) ** 2 <= 1) layers.push([255, 255, 255, 1]);
  }
  for (const cx of [0.3, 0.7]) {
    if (((x - cx) / 0.05) ** 2 + ((y - 0.58) / 0.03) ** 2 <= 1) layers.push(COLORS.cheek);
  }
  let r = 0, g = 0, b = 0, a = 0;
  for (const [lr, lg, lb, la] of layers) {
    // "over" compositing with straight (non-premultiplied) alpha
    const outA = la + a * (1 - la);
    if (outA === 0) continue;
    r = (lr * la + r * a * (1 - la)) / outA;
    g = (lg * la + g * a * (1 - la)) / outA;
    b = (lb * la + b * a * (1 - la)) / outA;
    a = outA;
  }
  return [r, g, b, a];
}

function render(size) {
  const px = Buffer.alloc(size * size * 4);
  const S = 4;
  for (let py = 0; py < size; py++) {
    for (let pxl = 0; pxl < size; pxl++) {
      let r = 0, g = 0, b = 0, a = 0;
      for (let sy = 0; sy < S; sy++) {
        for (let sx = 0; sx < S; sx++) {
          const [cr, cg, cb, ca] = paint((pxl + (sx + 0.5) / S) / size, (py + (sy + 0.5) / S) / size);
          r += cr * ca; g += cg * ca; b += cb * ca; a += ca;
        }
      }
      const i = (py * size + pxl) * 4;
      px[i] = a ? Math.round(r / a) : 0;
      px[i + 1] = a ? Math.round(g / a) : 0;
      px[i + 2] = a ? Math.round(b / a) : 0;
      px[i + 3] = Math.round((a / (S * S)) * 255);
    }
  }
  return px;
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(buf) {
  let c = 0xffffffff;
  for (const byte of buf) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}
function png(size, rgba) {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(size, 0);
  header.writeUInt32BE(size, 4);
  header[8] = 8; // bit depth
  header[9] = 6; // colour type RGBA
  const rows = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) {
    rows[y * (size * 4 + 1)] = 0; // filter: none
    rgba.copy(rows, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(rows, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}
// .ico = a small directory followed by PNG images (supported since Windows Vista).
function ico(images) {
  const head = Buffer.alloc(6);
  head.writeUInt16LE(0, 0);
  head.writeUInt16LE(1, 2);
  head.writeUInt16LE(images.length, 4);
  let offset = 6 + 16 * images.length;
  const entries = images.map(({ size, data }) => {
    const e = Buffer.alloc(16);
    e[0] = size >= 256 ? 0 : size;
    e[1] = size >= 256 ? 0 : size;
    e.writeUInt16LE(1, 4);
    e.writeUInt16LE(32, 6);
    e.writeUInt32LE(data.length, 8);
    e.writeUInt32LE(offset, 12);
    offset += data.length;
    return e;
  });
  return Buffer.concat([head, ...entries, ...images.map((i) => i.data)]);
}

mkdirSync(OUT, { recursive: true });
const pngOf = (size) => png(size, render(size));
writeFileSync(join(OUT, "32x32.png"), pngOf(32));
writeFileSync(join(OUT, "128x128.png"), pngOf(128));
writeFileSync(join(OUT, "128x128@2x.png"), pngOf(256));
writeFileSync(join(OUT, "icon.png"), pngOf(512));
writeFileSync(join(OUT, "icon.ico"), ico([16, 24, 32, 48, 64, 128, 256].map((size) => ({ size, data: pngOf(size) }))));
console.log(`Icons written to ${OUT}`);
