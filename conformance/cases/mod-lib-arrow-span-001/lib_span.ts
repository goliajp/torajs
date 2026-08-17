// The padding keeps this file longer than main.ts, so the arrow's
// recorded span (near the file's end) indexes past the entry
// source's length — the shape that made intern_fn_source slice out
// of bounds before the lib expr-span reset.
export const pad = "unused padding so the interesting spans land deep into this file";
export class K {
  m() {
    const pick = () => 99;
    return pick();
  }
}
export const viaArrow = ((n: number) => n * 2)(21);
