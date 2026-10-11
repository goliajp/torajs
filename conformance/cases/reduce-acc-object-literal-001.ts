// a reducer whose accumulator is an object type and whose body returns
// a fresh object literal, with no return annotation
const ys = [1, 2, 3];
const o = ys.reduce((acc: { sum: number; n: number }, x) => ({ sum: acc.sum + x, n: acc.n + 1 }), { sum: 0, n: 0 });
console.log(o.sum, o.n);

const words = ["a", "bb", "a", "ccc"];
const w = words.reduce((acc: { count: number; longest: string }, s) => {
  const longest = s.length > acc.longest.length ? s : acc.longest;
  return { count: acc.count + 1, longest: longest };
}, { count: 0, longest: "" });
console.log(w.count, w.longest);

const r = ys.reduceRight((acc: { seq: string; last: number }, x: number) => ({ seq: acc.seq + x, last: x }), { seq: "", last: -1 });
console.log(r.seq, r.last);

type Acc = { sum: number; n: number };
const a = ys.reduce((acc: Acc, x: number) => ({ sum: acc.sum + x * 10, n: acc.n + 1 }), { sum: 0, n: 0 });
console.log(a.sum, a.n);

function stats(xs: number[]): { min: number; max: number } {
  const seed = { min: Infinity, max: -Infinity };
  const s = xs.reduce((acc: { min: number; max: number }, x: number) => ({
    min: x < acc.min ? x : acc.min,
    max: x > acc.max ? x : acc.max,
  }), seed);
  console.log(seed.min, seed.max);
  return s;
}
const st = stats([4, 9, 1, 7]);
console.log(st.min, st.max);

const nested = ys.reduce((acc: { inner: { t: number } }, x: number) => ({ inner: { t: acc.inner.t + x } }), { inner: { t: 100 } });
console.log(nested.inner.t);

let total = 0;
for (let i = 0; i < 2000; i++) {
  const q = ys.reduce((acc: { sum: number; n: number }, x) => ({ sum: acc.sum + x + i, n: acc.n + 1 }), { sum: 0, n: 0 });
  const b = ys.reduce((acc: Acc, x: number) => ({ sum: acc.sum + x, n: acc.n + 1 }), { sum: i, n: 0 });
  total += q.sum + b.sum + b.n;
}
console.log(total);
