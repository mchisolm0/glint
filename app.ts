// Glint's TypeScript half: the library (keep, list, copy, delete), saved annotations, the bridge to the Mac's
// overlays and panels (modules/apple), and the editor's canvas. data/src/lib.rs takes the pictures and recordings.
import { write, writeText, ClipboardItem } from '@exact/clipboard';
import type { Args, Ctx2D, Frame, NativeModule, Result, Storage, TypeScriptAnswer, TypeScriptSources } from './app.contract.d.ts';

// One annotation, as app.contract's Mark shape (shapes.contract).
type Mark = Args<'saveEdit'>[1][number];

// The staging folders are data/src/lib.rs's STAGING (/private/tmp/glint on macOS, /tmp/glint on Linux).
export const grants = [
  'device.global-shortcuts',
  'fs.read app:/data/captures',
  'fs.write app:/data/captures',
  'fs.read /private/tmp/glint',
  'fs.write /private/tmp/glint',
  'fs.read /tmp/glint',
  'fs.write /tmp/glint',
].join('\n');

const folder = 'app:/data/captures';
// An original: glint-<taken>-<w>x<h>.png, or glint-<taken>-<w>x<h>-<ms>.mp4|gif for a recording (docs/design.md).
const original = /^(glint-(\d+)-(\d+)x(\d+)(?:-(\d+))?)\.(png|mp4|gif)$/;
// An annotated render, versioned so an `image` never shows a stale one: <stem>.edit-<n>.png.
const render = /^(.+)\.edit-(\d+)\.png$/;

const ok = { ok: true, error: '' };
const refused = (e: unknown) => ({ ok: false, error: e instanceof Error ? e.message : String(e) });

// The Swift module on the Mac; null where the host has none (Linux, the web, the agent's stand-in without one).
const mac = (native?: NativeModule | null) => (native?.available ? native : null);

async function list(storage: Storage) {
  return storage.fs.readdir(folder).catch((e: { code?: string }) => {
    if (e.code === 'ENOENT') return [] as string[];
    throw e;
  });
}

// One library entry from its file name and the folder's other names (its mosaic and latest render).
function entry(name: string, names: readonly string[]) {
  const m = original.exec(name);
  if (!m) return null;
  const [, stem, taken, width, height, duration, kind] = m;
  const latest = names
    .flatMap((n) => {
      const r = render.exec(n);
      return r && r[1] === stem ? [{ n, at: Number(r[2]) }] : [];
    })
    .sort((a, b) => b.at - a.at)[0];
  const path = `${folder}/${name}`;
  return {
    path,
    name,
    stem,
    kind,
    taken: Number(taken),
    width: Number(width),
    height: Number(height),
    duration: Number(duration ?? 0),
    shown: latest ? `${folder}/${latest.n}` : path,
    mosaic: names.includes(`${stem}.mosaic.png`) ? `${folder}/${stem}.mosaic.png` : '',
    edited: Boolean(latest),
  };
}

// Moves a staged file into the library: a rename on the Mac, the bytes through here elsewhere.
async function adopt(from: string, to: string, storage: Storage, native?: NativeModule | null) {
  const swift = mac(native);
  if (swift) {
    await swift.later({ op: 'adopt', from, to });
    return;
  }
  await storage.fs.writeFile(to, await storage.fs.readFile(from));
  await storage.fs.rm(from);
}

async function nativeReply(native: NativeModule | null | undefined, request: Record<string, unknown>) {
  const swift = mac(native);
  if (!swift) return refused('Not on this platform yet.');
  try {
    await swift.later(request);
    return ok;
  } catch (e) {
    return refused(e);
  }
}

const sources: TypeScriptSources = {
  captures: async (_args, _store, storage) => {
    const names = await list(storage);
    return names.flatMap((n) => entry(n, names) ?? []).sort((a, b) => b.taken - a.taken);
  },
  keepCapture: async ([staged, taken, width, height, kind, duration, mosaic], _store, storage, native) => {
    const stem = `glint-${taken}-${width}x${height}${kind === 'png' ? '' : `-${duration}`}`;
    const name = `${stem}.${kind}`;
    await storage.fs.mkdir(folder);
    await adopt(staged, `${folder}/${name}`, storage, native);
    if (mosaic) await adopt(mosaic, `${folder}/${stem}.mosaic.png`, storage, native);
    const names = await list(storage);
    const kept = entry(name, names);
    if (!kept) throw new Error(`kept an unlisted name: ${name}`);
    return kept;
  },
  copyCapture: async ([path, kind], _store, _storage, native) => {
    if (mac(native)) return nativeReply(native, { op: 'copy', path, kind });
    try {
      await write([new ClipboardItem({ 'image/png': path })]);
      return ok;
    } catch (e) {
      return refused(e);
    }
  },
  copyText: async ([text]) => {
    try {
      await writeText(text);
      return ok;
    } catch (e) {
      return refused(e);
    }
  },
  deleteCapture: async ([path, stem], _store, storage) => {
    try {
      const names = await list(storage);
      await storage.fs.rm(path);
      for (const n of names.filter((n) => n.startsWith(`${stem}.`))) await storage.fs.rm(`${folder}/${n}`);
      return ok;
    } catch (e) {
      return refused(e);
    }
  },
  pick: async ([mode], _store, _storage, native) => {
    const swift = mac(native);
    if (!swift) throw new Error('No area picker on this platform yet.');
    return (await swift.later({ op: 'pick', mode })) as Result<'pick'>;
  },
  showQuick: ([path, kind], _store, _storage, native) => nativeReply(native, { op: 'quick', path, kind }),
  showHud: ([x, y, width, height], _store, _storage, native) => nativeReply(native, { op: 'hud', x, y, width, height }),
  hideHud: (_args, _store, _storage, native) => nativeReply(native, { op: 'hud', hide: true }),
  pinCapture: ([path], _store, _storage, native) => nativeReply(native, { op: 'pin', path }),
  revealCapture: ([path], _store, _storage, native) => nativeReply(native, { op: 'reveal', path }),
  // What a floating panel asked for; the module calls `changed("glint.requests")` after each.
  requests: (_args, _store, _storage, native) => {
    const swift = mac(native);
    if (!swift) return { seq: 0, kind: '', path: '' };
    swift.watch('glint.requests');
    return swift.call({ op: 'requests' }) as Result<'requests'>;
  },
  openEdit: async ([path], _store, storage) => {
    const stem = path.slice(folder.length + 1).replace(/\.png$/, '');
    const none = { path, marks: [], background: false, cx: 0, cy: 0, cw: 0, ch: 0 };
    try {
      const saved = JSON.parse(new TextDecoder().decode(await storage.fs.readFile(`${folder}/${stem}.edit.json`)));
      return { ...none, ...saved, path };
    } catch (e) {
      if ((e as { code?: string }).code === 'ENOENT') return none;
      throw e;
    }
  },
  // Renders the editor's export canvas (the marks at the picture's own pixels, crop and background included) and keeps its marks to edit again.
  saveEdit: async ([path, marks, background, cx, cy, cw, ch], _store, storage) => {
    const name = path.slice(folder.length + 1);
    const stem = name.replace(/\.png$/, '');
    const before = await list(storage);
    const version = Math.max(0, ...before.flatMap((n) => (render.exec(n)?.[1] === stem ? [Number(render.exec(n)?.[2])] : []))) + 1;
    const out = `${folder}/${stem}.edit-${version}.png`;
    await captureCanvas('export', out);
    await storage.fs.writeFile(`${folder}/${stem}.edit.json`, new TextEncoder().encode(JSON.stringify({ marks: marks.map(asMark), background, cx, cy, cw, ch })));
    for (const n of before) if (render.exec(n)?.[1] === stem && n !== out.slice(folder.length + 1)) await storage.fs.rm(`${folder}/${n}`);
    const kept = entry(name, await list(storage));
    if (!kept) throw new Error(`saved an unlisted capture: ${name}`);
    return kept;
  },
};

export const answer: TypeScriptAnswer = (source, args, store, storage, native) => sources[source](args, store, storage, native);

const fields = ['kind', 'color', 'size', 'x1', 'y1', 'x2', 'y2', 'points', 'text', 'n'] as const;
// A Mark as data code receives it: an object from storage, or a positional record (the shape's field order) from
// Contract, as canvas arguments and mutation arguments arrive.
function asMark(m: Mark | readonly unknown[]): Mark {
  if (!Array.isArray(m)) return m as Mark;
  return Object.fromEntries(fields.map((f, i) => [f, m[i]])) as Mark;
}

// The editor's canvases, `annotate` on screen and `export` while saving: (base, mosaic, marks, draft, drawing, vx, vy, vw, vh, pad, unit).
type Scene = [string, string, readonly (Mark | readonly unknown[])[], Mark | readonly unknown[], boolean, number, number, number, number, number, number];
export const surfaces: Record<string, number> = { annotate: 11, export: 11 };

export function draw(_surface: string, args: Scene, ctx: Ctx2D, frame: Frame): boolean {
  const [base, mosaic, rawMarks, rawDraft, drawing, vx, vy, vw, vh, pad, unit] = args;
  const marks = rawMarks.map(asMark);
  const draft = asMark(rawDraft);
  const bw = vw + 2 * pad;
  const bh = vh + 2 * pad;
  ctx.reset();
  // The context draws in the canvas's CSS pixels; the scene is in the picture's.
  ctx.scale(frame.width / bw, frame.height / bh);
  if (pad > 0) backdrop(ctx, bw, bh, pad, unit);
  ctx.save();
  ctx.translate(pad - vx, pad - vy);
  ctx.beginPath();
  if (pad > 0) ctx.roundRect(vx, vy, vw, vh, 10 * unit);
  else ctx.rect(vx, vy, vw, vh);
  ctx.clip();
  if (base) ctx.drawImage(base, vx, vy, vw, vh, vx, vy, vw, vh);
  const all = drawing && draft.kind !== 'crop' ? [...marks, draft] : marks;
  spotlight(ctx, all.filter((m) => m.kind === 'spotlight'), vx, vy, vw, vh);
  for (const m of all) if (m.kind !== 'spotlight') mark(ctx, m, mosaic, vx + vw, vy + vh);
  if (drawing && draft.kind === 'crop') cropGuide(ctx, draft, vx, vy, vw, vh, unit);
  ctx.restore();
  return false;
}

// The Background option: a soft gradient with the picture raised on it.
function backdrop(ctx: Ctx2D, bw: number, bh: number, pad: number, unit: number) {
  const g = ctx.createLinearGradient(0, 0, bw, bh);
  g.addColorStop(0, '#5b6cf9');
  g.addColorStop(1, '#c86dd7');
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, bw, bh);
  ctx.save();
  ctx.shadowColor = 'rgba(0, 0, 0, 0.35)';
  ctx.shadowBlur = 30 * unit;
  ctx.shadowOffsetY = 8 * unit;
  ctx.fillStyle = '#000';
  ctx.beginPath();
  ctx.roundRect(pad, pad, bw - 2 * pad, bh - 2 * pad, 10 * unit);
  ctx.fill();
  ctx.restore();
}

const box = (m: Mark) => ({ x: Math.min(m.x1, m.x2), y: Math.min(m.y1, m.y2), w: Math.abs(m.x2 - m.x1), h: Math.abs(m.y2 - m.y1) });

// Every spotlight at once: the picture dimmed but for their boxes.
function spotlight(ctx: Ctx2D, lights: readonly Mark[], vx: number, vy: number, vw: number, vh: number) {
  if (lights.length === 0) return;
  ctx.save();
  ctx.beginPath();
  ctx.rect(vx, vy, vw, vh);
  for (const m of lights) {
    const b = box(m);
    ctx.roundRect(b.x, b.y, b.w, b.h, Math.min(12, b.w / 4, b.h / 4));
  }
  ctx.fillStyle = 'rgba(0, 0, 0, 0.55)';
  ctx.fill('evenodd');
  ctx.restore();
}

function points(m: Mark) {
  return m.points.split(' ').flatMap((p: string) => {
    const [x, y] = p.split(',').map(Number);
    return Number.isFinite(x) && Number.isFinite(y) ? [{ x, y }] : [];
  });
}

function polyline(ctx: Ctx2D, m: Mark) {
  const ps = points(m);
  if (ps.length === 0) return;
  ctx.beginPath();
  ctx.moveTo(ps[0].x, ps[0].y);
  for (const p of ps.slice(1)) ctx.lineTo(p.x, p.y);
  if (ps.length === 1) ctx.lineTo(ps[0].x + 0.1, ps[0].y);
  ctx.stroke();
}

function mark(ctx: Ctx2D, m: Mark, mosaic: string, right: number, bottom: number) {
  const s = m.size;
  ctx.save();
  ctx.strokeStyle = m.color;
  ctx.fillStyle = m.color;
  ctx.lineWidth = s;
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';
  const raised = () => {
    ctx.shadowColor = 'rgba(0, 0, 0, 0.3)';
    ctx.shadowBlur = s * 1.5;
    ctx.shadowOffsetY = s * 0.25;
  };
  switch (m.kind) {
    case 'arrow':
      raised();
      arrow(ctx, m);
      break;
    case 'rect': {
      raised();
      const b = box(m);
      ctx.beginPath();
      ctx.roundRect(b.x, b.y, b.w, b.h, s);
      ctx.stroke();
      break;
    }
    case 'ellipse': {
      raised();
      const b = box(m);
      ctx.beginPath();
      ctx.ellipse(b.x + b.w / 2, b.y + b.h / 2, b.w / 2, b.h / 2, 0, 0, Math.PI * 2);
      ctx.stroke();
      break;
    }
    case 'line':
      raised();
      ctx.beginPath();
      ctx.moveTo(m.x1, m.y1);
      ctx.lineTo(m.x2, m.y2);
      ctx.stroke();
      break;
    case 'pen':
      raised();
      polyline(ctx, m);
      break;
    case 'marker':
      ctx.globalAlpha = 0.4;
      ctx.lineWidth = s * 4;
      ctx.lineCap = 'square';
      polyline(ctx, m);
      break;
    case 'pixelate': {
      const b = box(m);
      ctx.beginPath();
      ctx.rect(b.x, b.y, b.w, b.h);
      ctx.clip();
      if (mosaic) {
        ctx.imageSmoothingEnabled = false;
        ctx.drawImage(mosaic, 0, 0, right, bottom);
      } else {
        ctx.fillStyle = '#1c1c1e';
        ctx.fillRect(b.x, b.y, b.w, b.h);
      }
      break;
    }
    case 'text': {
      const px = s * 3.5;
      ctx.font = `600 ${px}px -apple-system, system-ui, sans-serif`;
      ctx.textBaseline = 'top';
      ctx.lineWidth = Math.max(2, s * 0.6);
      ctx.strokeStyle = isLight(m.color) ? 'rgba(0, 0, 0, 0.7)' : 'rgba(255, 255, 255, 0.85)';
      ctx.strokeText(m.text, m.x1, m.y1 - s * 2);
      ctx.fillText(m.text, m.x1, m.y1 - s * 2);
      break;
    }
    case 'step': {
      const r = s * 2.2 + 10;
      raised();
      ctx.beginPath();
      ctx.arc(m.x1, m.y1, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.shadowColor = 'transparent';
      ctx.fillStyle = isLight(m.color) ? '#000' : '#fff';
      ctx.font = `700 ${r * 1.15}px -apple-system, system-ui, sans-serif`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.fillText(String(m.n), m.x1, m.y1 + r * 0.05);
      break;
    }
  }
  ctx.restore();
}

// A tapered arrow, wide at the head, as Shottr and CleanShot draw them.
function arrow(ctx: Ctx2D, m: Mark) {
  const dx = m.x2 - m.x1;
  const dy = m.y2 - m.y1;
  const len = Math.hypot(dx, dy);
  if (len < 1) return;
  const ux = dx / len;
  const uy = dy / len;
  const nx = -uy;
  const ny = ux;
  const w = m.size;
  const head = Math.min(len * 0.5, w * 4 + 8);
  const half = w * 1.9 + 3;
  const neck = w * 0.7;
  const tail = w * 0.2;
  const bx = m.x2 - ux * head;
  const by = m.y2 - uy * head;
  ctx.beginPath();
  ctx.moveTo(m.x1 + nx * tail, m.y1 + ny * tail);
  ctx.lineTo(bx + nx * neck, by + ny * neck);
  ctx.lineTo(bx + nx * half, by + ny * half);
  ctx.lineTo(m.x2, m.y2);
  ctx.lineTo(bx - nx * half, by - ny * half);
  ctx.lineTo(bx - nx * neck, by - ny * neck);
  ctx.lineTo(m.x1 - nx * tail, m.y1 - ny * tail);
  ctx.closePath();
  ctx.fill();
}

// The crop being dragged: everything outside it dimmed, a dashed edge on it.
function cropGuide(ctx: Ctx2D, m: Mark, vx: number, vy: number, vw: number, vh: number, unit: number) {
  const b = box(m);
  ctx.save();
  ctx.beginPath();
  ctx.rect(vx, vy, vw, vh);
  ctx.rect(b.x, b.y, b.w, b.h);
  ctx.fillStyle = 'rgba(0, 0, 0, 0.5)';
  ctx.fill('evenodd');
  ctx.setLineDash([6 * unit, 4 * unit]);
  ctx.lineWidth = 1.5 * unit;
  ctx.strokeStyle = '#fff';
  ctx.strokeRect(b.x, b.y, b.w, b.h);
  ctx.restore();
}

function isLight(hex: string) {
  const n = Number.parseInt(hex.slice(1), 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  return 0.299 * r + 0.587 * g + 0.114 * b > 160;
}
