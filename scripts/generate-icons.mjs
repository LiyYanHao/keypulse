import { spawnSync } from "node:child_process";
import { deflateSync } from "node:zlib";
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const iconDir = path.join(root, "src-tauri", "icons");
const iconsetDir = path.join(iconDir, "icon.iconset");

fs.mkdirSync(iconDir, { recursive: true });
fs.rmSync(iconsetDir, { recursive: true, force: true });
fs.mkdirSync(iconsetDir, { recursive: true });

const lerp = (a, b, t) => Math.round(a + (b - a) * t);
const clamp = (value, min, max) => Math.max(min, Math.min(max, value));

function crc32(buffer) {
  let crc = -1;
  for (const byte of buffer) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) {
      crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
    }
  }
  return (crc ^ -1) >>> 0;
}

function chunk(type, data) {
  const typeBuffer = Buffer.from(type);
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length, 0);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([typeBuffer, data])), 0);
  return Buffer.concat([length, typeBuffer, data, crc]);
}

function encodePng(width, height, rgba) {
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y += 1) {
    const rowStart = y * (width * 4 + 1);
    raw[rowStart] = 0;
    rgba.copy(raw, rowStart + 1, y * width * 4, (y + 1) * width * 4);
  }

  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header[8] = 8;
  header[9] = 6;
  header[10] = 0;
  header[11] = 0;
  header[12] = 0;

  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

function blend(buffer, size, x, y, color, alpha = 1) {
  const ix = Math.round(x);
  const iy = Math.round(y);
  if (ix < 0 || iy < 0 || ix >= size || iy >= size || alpha <= 0) return;
  const offset = (iy * size + ix) * 4;
  const srcAlpha = clamp((color[3] / 255) * alpha, 0, 1);
  const dstAlpha = buffer[offset + 3] / 255;
  const outAlpha = srcAlpha + dstAlpha * (1 - srcAlpha);
  if (outAlpha <= 0) return;
  for (let channel = 0; channel < 3; channel += 1) {
    const src = color[channel] / 255;
    const dst = buffer[offset + channel] / 255;
    buffer[offset + channel] = Math.round(
      ((src * srcAlpha + dst * dstAlpha * (1 - srcAlpha)) / outAlpha) * 255,
    );
  }
  buffer[offset + 3] = Math.round(outAlpha * 255);
}

function roundedRectMask(px, py, x, y, w, h, radius) {
  const cx = clamp(px, x + radius, x + w - radius);
  const cy = clamp(py, y + radius, y + h - radius);
  const dx = px - cx;
  const dy = py - cy;
  return dx * dx + dy * dy <= radius * radius;
}

function paintRoundedRect(buffer, size, rect, color) {
  const [x, y, w, h, radius] = rect;
  const minX = Math.max(0, Math.floor(x));
  const minY = Math.max(0, Math.floor(y));
  const maxX = Math.min(size, Math.ceil(x + w));
  const maxY = Math.min(size, Math.ceil(y + h));
  for (let py = minY; py < maxY; py += 1) {
    for (let px = minX; px < maxX; px += 1) {
      if (roundedRectMask(px + 0.5, py + 0.5, x, y, w, h, radius)) {
        blend(buffer, size, px, py, color, color[3] / 255);
      }
    }
  }
}

function paintLine(buffer, size, points, width, color) {
  for (let i = 0; i < points.length - 1; i += 1) {
    const [x1, y1] = points[i];
    const [x2, y2] = points[i + 1];
    const minX = Math.max(0, Math.floor(Math.min(x1, x2) - width));
    const maxX = Math.min(size, Math.ceil(Math.max(x1, x2) + width));
    const minY = Math.max(0, Math.floor(Math.min(y1, y2) - width));
    const maxY = Math.min(size, Math.ceil(Math.max(y1, y2) + width));
    const dx = x2 - x1;
    const dy = y2 - y1;
    const lengthSq = dx * dx + dy * dy || 1;
    for (let y = minY; y < maxY; y += 1) {
      for (let x = minX; x < maxX; x += 1) {
        const t = clamp(((x - x1) * dx + (y - y1) * dy) / lengthSq, 0, 1);
        const cx = x1 + t * dx;
        const cy = y1 + t * dy;
        const dist = Math.hypot(x - cx, y - cy);
        const edge = width / 2;
        const alpha = clamp(edge + 1.2 - dist, 0, 1);
        blend(buffer, size, x, y, color, alpha);
      }
    }
  }
}

function renderIcon(size) {
  const buffer = Buffer.alloc(size * size * 4);
  const radius = size * 0.22;

  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      if (!roundedRectMask(x + 0.5, y + 0.5, 0, 0, size, size, radius)) continue;
      const diagonal = (x + y) / (size * 2);
      const glow = Math.max(0, 1 - Math.hypot(x - size * 0.72, y - size * 0.2) / (size * 0.62));
      const r = lerp(14, 38, diagonal) + Math.round(glow * 12);
      const g = lerp(26, 104, diagonal) + Math.round(glow * 20);
      const b = lerp(55, 214, diagonal) + Math.round(glow * 18);
      const offset = (y * size + x) * 4;
      buffer[offset] = clamp(r, 0, 255);
      buffer[offset + 1] = clamp(g, 0, 255);
      buffer[offset + 2] = clamp(b, 0, 255);
      buffer[offset + 3] = 255;
    }
  }

  paintRoundedRect(buffer, size, [size * 0.15, size * 0.66, size * 0.7, size * 0.18, size * 0.05], [
    255,
    255,
    255,
    28,
  ]);

  const keyY = size * 0.69;
  const keyW = size * 0.12;
  const gap = size * 0.035;
  for (let index = 0; index < 4; index += 1) {
    paintRoundedRect(buffer, size, [size * 0.21 + index * (keyW + gap), keyY, keyW, size * 0.11, size * 0.026], [
      219,
      234,
      254,
      178,
    ]);
  }
  paintRoundedRect(buffer, size, [size * 0.36, size * 0.82, size * 0.3, size * 0.055, size * 0.022], [
    219,
    234,
    254,
    138,
  ]);

  const pulse = [
    [size * 0.13, size * 0.49],
    [size * 0.26, size * 0.49],
    [size * 0.34, size * 0.36],
    [size * 0.44, size * 0.64],
    [size * 0.53, size * 0.26],
    [size * 0.63, size * 0.49],
    [size * 0.88, size * 0.49],
  ];
  paintLine(buffer, size, pulse, size * 0.078, [4, 18, 38, 80]);
  paintLine(buffer, size, pulse, size * 0.052, [236, 253, 245, 245]);
  paintLine(buffer, size, pulse, size * 0.022, [34, 211, 238, 255]);

  return encodePng(size, size, buffer);
}

function writeIcon(name, size) {
  const png = renderIcon(size);
  fs.writeFileSync(path.join(iconDir, name), png);
  return png;
}

const files = [
  ["32x32.png", 32],
  ["128x128.png", 128],
  ["128x128@2x.png", 256],
  ["icon.png", 1024],
];
for (const [name, size] of files) writeIcon(name, size);

const iconset = [
  ["icon_16x16.png", 16],
  ["icon_16x16@2x.png", 32],
  ["icon_32x32.png", 32],
  ["icon_32x32@2x.png", 64],
  ["icon_128x128.png", 128],
  ["icon_128x128@2x.png", 256],
  ["icon_256x256.png", 256],
  ["icon_256x256@2x.png", 512],
  ["icon_512x512.png", 512],
  ["icon_512x512@2x.png", 1024],
];
for (const [name, size] of iconset) {
  fs.writeFileSync(path.join(iconsetDir, name), renderIcon(size));
}

const icoSizes = [16, 32, 48, 64, 128, 256].map((size) => ({
  size,
  data: renderIcon(size),
}));
const header = Buffer.alloc(6);
header.writeUInt16LE(0, 0);
header.writeUInt16LE(1, 2);
header.writeUInt16LE(icoSizes.length, 4);
const entries = [];
let offset = header.length + icoSizes.length * 16;
for (const icon of icoSizes) {
  const entry = Buffer.alloc(16);
  entry[0] = icon.size === 256 ? 0 : icon.size;
  entry[1] = icon.size === 256 ? 0 : icon.size;
  entry[2] = 0;
  entry[3] = 0;
  entry.writeUInt16LE(1, 4);
  entry.writeUInt16LE(32, 6);
  entry.writeUInt32LE(icon.data.length, 8);
  entry.writeUInt32LE(offset, 12);
  offset += icon.data.length;
  entries.push(entry);
}
fs.writeFileSync(path.join(iconDir, "icon.ico"), Buffer.concat([header, ...entries, ...icoSizes.map((icon) => icon.data)]));

const icnsResult = spawnSync("iconutil", ["-c", "icns", iconsetDir, "-o", path.join(iconDir, "icon.icns")], {
  stdio: "inherit",
});
if (icnsResult.status !== 0) {
  console.warn("iconutil is unavailable; skipped macOS .icns generation");
} else {
  fs.rmSync(iconsetDir, { recursive: true, force: true });
}

console.log(`Generated KeyPulse icons in ${iconDir}`);
