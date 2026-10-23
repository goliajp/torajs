// Promise.race / Promise.any over an escaped array at top level, through
// a typed parameter, through a promise binding, into `+`, into string and
// boolean slots, and beside a typed source that keeps its number lane.
function strFirst(): any { return ["s", Promise.resolve(1)]; }
function boolFirst(): any { return [true, Promise.resolve(1)]; }
const top = strFirst() as Promise<number>[];
const tr = await Promise.race(top);
console.log("top race", tr, typeof tr);
const ta = await Promise.any(top);
console.log("top any", ta, typeof ta);
async function first(xs: Promise<number>[]): Promise<number> {
  const v = await Promise.race(xs);
  return v;
}
async function viaParam() { console.log("param", await first(strFirst())); }
await viaParam();
async function main() {
  const a: Promise<number>[] = strFirst();
  const p = Promise.race(a);
  const v = await p;
  console.log("alias", v, typeof v);
  let t = 0;
  t = t + (await Promise.any(a));
  console.log("plus", t, typeof t);
  const typed: Promise<number>[] = [Promise.resolve(4.5), Promise.resolve(2)];
  const w = await Promise.race(typed);
  console.log("typed", w, w * 2, typeof w);
  const k = await Promise.any(typed);
  console.log("typed any", k + 1);
  const s: Promise<string>[] = boolFirst();
  const bs = await Promise.race(s);
  console.log("str slot", bs, typeof bs);
  const b: Promise<boolean>[] = strFirst();
  const bb = await Promise.any(b);
  console.log("bool slot", bb, typeof bb);
  console.log("direct", await Promise.race(a));
}
await main();
