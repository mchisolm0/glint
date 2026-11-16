// Prints the colour at the centre of the three squares in an agent screenshot:
// the red source canvas, the drawImage canvas (blue background = nothing drawn), the image node.
// Usage: bun check.mjs .exact/web.png
import { readFileSync } from 'node:fs';
import { inflateSync } from 'node:zlib';

const png = readFileSync(process.argv[2]);
const width = png.readUInt32BE(16), depth = png[24], colour = png[25];
if (depth !== 8 || (colour !== 2 && colour !== 6)) throw new Error(`unsupported PNG: depth ${depth}, colour type ${colour}`);
const chunks = [];
for (let at = 8; at < png.length; ) {
  const len = png.readUInt32BE(at), type = png.toString('latin1', at + 4, at + 8);
  if (type === 'IDAT') chunks.push(png.subarray(at + 8, at + 8 + len));
  at += 12 + len;
}
const raw = inflateSync(Buffer.concat(chunks)), bpp = colour === 6 ? 4 : 3, stride = width * bpp;
const rows = [];
for (let y = 0, prev = Buffer.alloc(stride); y * (stride + 1) < raw.length; y++) {
  const f = raw[y * (stride + 1)], line = Buffer.from(raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1)));
  for (let i = 0; i < stride; i++) {
    const a = i >= bpp ? line[i - bpp] : 0, b = prev[i], c = i >= bpp ? prev[i - bpp] : 0;
    const p = a + b - c, pr = Math.abs(p - a) <= Math.abs(p - b) && Math.abs(p - a) <= Math.abs(p - c) ? a : Math.abs(p - b) <= Math.abs(p - c) ? b : c;
    line[i] = (line[i] + [0, a, b, (a + b) >> 1, pr][f]) & 255;
  }
  rows.push(line);
  prev = line;
}
const hex = (x, y) => '#' + [...rows[y].subarray(x * bpp, x * bpp + 3)].map((v) => v.toString(16).padStart(2, '0')).join('');
for (const [name, x] of [['red canvas', 66], ['drawImage canvas', 174], ['image node', 282]]) console.log(`${name.padEnd(17)} ${hex(x, 100)}`);
