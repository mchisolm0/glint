import type { Answer, Ctx2D, Frame } from './app.contract.d.ts';

export const grants = 'fs.read app:/data\nfs.write app:/data';

const PATH = 'app:/data/red.png';

export const answer: Answer = (async (source: string) => {
  if (source !== 'writeRed') throw new Error(source);
  const c = await captureCanvas('red', PATH);
  console.log(`captured ${c.path} ${c.width}x${c.height} ${c.type}`);
  return { path: c.path };
}) as unknown as Answer;

export const surfaces: Record<string, number> = { red: 0, show: 1, record: 1 };

export function draw(surface: string, args: unknown[], ctx: Ctx2D, frame: Frame): boolean {
  if (surface === 'red') {
    ctx.fillStyle = '#ff0000';
    ctx.fillRect(0, 0, frame.width, frame.height);
  } else if (surface === 'show') {
    // The image node beside this shows the same path; this drawImage draws nothing.
    console.log(`show ${String(args[0])} causes=${frame.causes.join(',')}`);
    ctx.reset();
    ctx.drawImage(args[0] as string, 0, 0, frame.width, frame.height);
  } else if (surface === 'record') {
    // "Also": a Written record arrives as a positional array, not { path }.
    console.log(`record arg ${JSON.stringify(args[0])}`);
  }
  return false;
}
