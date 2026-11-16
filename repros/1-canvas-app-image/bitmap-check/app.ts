import type { Answer, Ctx2D, Frame } from './app.contract.d.ts';

export const grants = '';
export const answer: Answer = (async (source: string) => { throw new Error(source); }) as unknown as Answer;
export const surfaces: Record<string, number> = { dot: 0 };
export function draw(_surface: string, _args: unknown[], ctx: Ctx2D, frame: Frame): boolean {
  ctx.fillRect(0, 0, frame.width, frame.height);
  return false;
}
