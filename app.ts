// Glint's TypeScript half: the capture list, the clipboard and the area picker. data/src/lib.rs takes the pictures.
import { write, ClipboardItem } from '@exact/clipboard';
import type { Result, TypeScriptAnswer, TypeScriptSources } from './app.contract.d.ts';

// The staging folder is data/src/lib.rs's STAGING (Linux: /tmp/glint).
export const grants = 'device.global-shortcuts\nfs.read app:/data/captures\nfs.write app:/data/captures\nfs.read /private/tmp/glint\nfs.write /private/tmp/glint';

const folder = 'app:/data/captures';
// glint-<ms>-<w>x<h>.png, as data/src/lib.rs names them.
const named = /^glint-(\d+)-(\d+)x(\d+)\.png$/;

const ok = { ok: true, error: '' };
const refused = (e: unknown) => ({ ok: false, error: e instanceof Error ? e.message : String(e) });

const sources: TypeScriptSources = {
  captures: async (_args, _store, storage) => {
    const names = await storage.fs.readdir(folder).catch((e: { code?: string }) => {
      if (e.code === 'ENOENT') return [];
      throw e;
    });
    return names
      .flatMap((name) => {
        const m = named.exec(name);
        return m ? [{ path: `${folder}/${name}`, name, taken: Number(m[1]), width: Number(m[2]), height: Number(m[3]) }] : [];
      })
      .sort((a, b) => b.taken - a.taken);
  },
  // Moves what Rust staged into app storage, named so `captures` can list it without reading it.
  keepCapture: async ([staged, taken, width, height], _store, storage) => {
    const name = `glint-${taken}-${width}x${height}.png`;
    const path = `${folder}/${name}`;
    await storage.fs.mkdir(folder);
    // copyFile refuses to cross from a native path into app storage, so the bytes go through here.
    await storage.fs.writeFile(path, await storage.fs.readFile(staged));
    await storage.fs.rm(staged);
    return { path, name, taken, width, height };
  },
  copyCapture: async ([path]) => {
    try {
      await write([new ClipboardItem({ 'image/png': path })]);
      return ok;
    } catch (e) {
      return refused(e);
    }
  },
  deleteCapture: async ([path], _store, storage) => {
    try {
      await storage.fs.rm(path);
      return ok;
    } catch (e) {
      return refused(e);
    }
  },
  // The Mac's overlay (modules/apple/AreaPicker.swift); elsewhere there is none yet.
  pickArea: async (_args, _store, _storage, native) => {
    if (!native?.available) throw new Error('No area picker on this platform yet.');
    return (await native.later({ op: 'pickArea' })) as Result<'pickArea'>;
  },
};

export const answer: TypeScriptAnswer = (source, args, store, storage, native) => sources[source](args, store, storage, native);
