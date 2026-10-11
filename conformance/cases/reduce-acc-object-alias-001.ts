// a reducer whose accumulator is named by an object type alias or an
// interface reads its fields the way an inline object type does
type Acc = { sum: number; n: number };
type Inner = { t: number; tag: string };
type Outer = { inner: Inner; k: number };
interface Span { lo: number; hi: number }
type Pt = { x: number };

const ys = [3, 1, 4, 1, 5];

const a = ys.reduce((acc: Acc, x: number) => ({ sum: acc.sum + x, n: acc.n + 1 }), { sum: 0, n: 0 });
console.log(a.sum, a.n);

const sp = ys.reduceRight((acc: Span, x: number) => ({
  lo: x < acc.lo ? x : acc.lo,
  hi: x > acc.hi ? x : acc.hi,
}), { lo: 99, hi: -99 });
console.log(sp.lo, sp.hi);

const o = ys.reduce((acc: Outer, x: number) => ({
  inner: { t: acc.inner.t + x, tag: acc.inner.tag + x },
  k: acc.k + 1,
}), { inner: { t: 0, tag: "" }, k: 0 });
console.log(o.inner.t, o.inner.tag, o.k);

// a return the sniff cannot type keeps the boxed accumulator
const m = ys.reduce((acc: Acc, x: number) => ({ sum: Math.max(acc.sum, x), n: acc.n + 1 }), { sum: 0, n: 0 });
console.log(m.sum, m.n);

// a type parameter spelled like the alias is not the alias
function first<Pt>(xs: Pt[]): Pt {
  return xs.reduce((acc: Pt, x: Pt) => acc, xs[0]);
}
const pt: Pt = { x: 7 };
console.log([1, 2].reduce((acc: number, y: number) => acc + y + pt.x, 0));
console.log(first(["p", "q"]));

const seed: Acc = { sum: 100, n: 0 };
let total = 0;
for (let i = 0; i < 2000; i++) {
  const b = ys.reduce((acc: Acc, x: number) => ({ sum: acc.sum + x + i, n: acc.n + 1 }), seed);
  total += b.sum + b.n;
}
console.log(total, seed.sum, seed.n);
