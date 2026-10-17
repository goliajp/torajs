// An `any` member of an object literal written through a receiver whose
// declared type is a generic type alias: the field the cast at the
// write checks keeps the fraction the value carries.
type T = { x: number };
type W<U> = { t: T; u: U };
type N<U> = { w: W<U> };
const vals: any[] = [JSON.parse('{"x":1.5}'), { x: 2 }, JSON.parse('{"x":-0.25}')];
const n: N<string> = { w: { t: { x: 0 }, u: "a" } };
let sum = 0;
for (const v of vals) {
  n.w = { t: v, u: "b" };
  sum += n.w.t.x;
  console.log(n.w.t.x, n.w.u);
}
const g: W<number> = { t: vals[0], u: 3 };
console.log(g.t.x + g.u);
let h: W<boolean> = { t: { x: 1 }, u: true };
h = { t: vals[2], u: false };
console.log(h.t.x, h.u, sum);
