// a named function boxed into the any lane by a consumer that only
// reads it (Object.is, a JSON.stringify replacer walk, a switch case,
// an any argument) or by one that keeps it (a join, an any binding,
// an any field, an array slot, an any return, an any assignment)
// answers its one fn object every time, which stays alive and keeps
// its expandos
function inc(n: number): number { return n + 1; }
function takeAny(v: any): number { return typeof v === "function" ? 1 : 0; }
function retAny(): any { return inc; }
function local(): number {
  let l: any = 0;
  l = inc;
  return l(0);
}
(inc as any).tag = 7;
const g: any = inc;
const h = inc;
const objs: any = {};
const keep: any[] = [];
let slot: any = 0;
let s = 0;
for (let i = 0; i < 100000; i++) {
  if (Object.is(inc, g)) s++;
  if (Object.is(g, inc)) s++;
  if (JSON.stringify(inc, (k: string, v: any) => v) === undefined) s++;
  switch (g) {
    case inc: s++; break;
    default: break;
  }
  switch (g) {
    case h: s++; break;
    default: break;
  }
  s += takeAny(inc);
  const r = objs.none ?? inc;
  s += r(0);
  const a: any = inc;
  s += a(0);
  const o: { f: any } = { f: inc };
  s += o.f(0);
  slot = inc;
  s += slot(0);
  s += retAny()(0);
  s += local();
  if (i % 1000 === 0) keep.push(inc);
}
console.log(s, keep.length, keep[99] === inc, retAny() === inc);
console.log(inc(1), inc === g, (inc as any).tag, (g as any).tag, g(41), slot === inc);
