// An `any` value in an object literal's member whose declared type is
// an object layout (or an array of them): the member takes the value
// as the slot's readers read it, for every way a literal meets its
// declared type (binding, reassignment, argument, return, class field,
// `as`, nested literal, module global).
type T = { x: number };
type W = { t: T };
type O = { t: T | undefined };
type N = { w: W };
type A = { ts: T[] };
class K {
  w: W = { t: { x: 0 } };
}
const d: any = {};
d.x = 4;
const vals: any[] = [{ x: 1 }, d, JSON.parse('{"x":7}')];
function arg(w: W) {
  return w.t.x;
}
function ret(v: any): W {
  return { t: v };
}
function tryit(f: () => string): string {
  try {
    return f();
  } catch (e) {
    return (e as Error).constructor.name;
  }
}
let g: W = { t: { x: 0 } };
const out: string[] = [];
for (const v of vals) {
  out.push(tryit(() => { const w: W = { t: v }; return String(w.t.x); }));
  out.push(tryit(() => { let w: W = { t: { x: 0 } }; w = { t: v }; return String(w.t.x); }));
  out.push(tryit(() => String(arg({ t: v }))));
  out.push(tryit(() => String(ret(v).t.x)));
  out.push(tryit(() => { const k = new K(); k.w = { t: v }; return String(k.w.t.x); }));
  out.push(tryit(() => String(({ t: v } as W).t.x)));
  out.push(tryit(() => { const n: N = { w: { t: v } }; return String(n.w.t.x); }));
  out.push(tryit(() => { g = { t: v }; return String(g.t.x); }));
  out.push(tryit(() => { const a: A = { ts: [v] }; return String(a.ts[0].x); }));
}
console.log(out.join(" "));
const u: any = undefined;
console.log(tryit(() => { const o: O = { t: u }; return String(o.t?.x); }));
console.log(tryit(() => { const w: W = { t: u }; return String(w.t.x); }));
const arr: any = [{ x: 2 }, d];
console.log(tryit(() => { const a: A = { ts: arr }; return a.ts.map((t) => t.x).join(","); }));
const top: W = { t: vals[1] };
console.log(top.t.x);
