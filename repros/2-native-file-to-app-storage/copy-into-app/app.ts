import type { Answer, Result, Sources, Storage } from './app.contract.d.ts';

// A native folder, as a Rust source's capture would write to (/tmp is a link on macOS; grants name the real path).
const NATIVE = '/private/tmp/exact-repro2';
export const grants = [
  `fs.read ${NATIVE}`,
  `fs.write ${NATIVE}`,
  'fs.read app:/data',
  'fs.write app:/data',
].join('\n');

// Runs each step and records "ok" or the refusal, so one probe shows every outcome.
async function probe(storage: Storage): Promise<string[]> {
  const lines: string[] = [];
  const step = async (name: string, work: () => Promise<unknown>) => {
    try {
      await work();
      lines.push(`${name}: ok`);
    } catch (e) {
      const { code, message } = e as { code?: string; message?: string };
      lines.push(`${name}: FAILED code=${code ?? '-'} message=${message ?? String(e)}`);
    }
  };
  const src = `${NATIVE}/shot.png`;
  await step('writeFile native', () => storage.fs.writeFile(src, new Uint8Array([0x89, 0x50, 0x4e, 0x47])));
  await step('readFile native', () => storage.fs.readFile(src));
  await step('writeFile app:/data', () => storage.fs.writeFile('app:/data/probe.txt', new TextEncoder().encode('hi')));
  await step('copyFile native -> app:/data', () => storage.fs.copyFile(src, 'app:/data/shot.png'));
  await step('rename native -> app:/data', () => storage.fs.rename(src, 'app:/data/shot.png'));
  await step('copyFile native -> native', () => storage.fs.copyFile(src, `${NATIVE}/copy.png`));
  await step('readFile + writeFile (workaround)', async () =>
    storage.fs.writeFile('app:/data/shot.png', await storage.fs.readFile(src)));
  return lines;
}

const sources: Sources = {
  probe: async (_args, _store, storage): Promise<Result<'probe'>> => ({ lines: await probe(storage) }),
};
export const answer: Answer = (source, args, store, storage, native) =>
  sources[source](args, store, storage, native);
