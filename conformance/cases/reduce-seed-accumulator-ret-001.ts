// A function that returns `xs.reduce(cb, seed)` returns the accumulator
// type: the seed's, or the callback's annotated accumulator parameter,
// not the array's element type.

function joinNums(xs: number[]) {
  return xs.reduce((a: string, x: number) => a + x, "");
}
const r = joinNums([1, 2]);
console.log(r, typeof r, r.length);

function joinRight(rs: number[]) {
  return rs.reduceRight((a: string, x: number) => a + x, "");
}
console.log(joinRight([1, 2, 3]), joinRight([4]).length);

function countLen(ss: string[]) {
  return ss.reduce((n, s) => n + s.length, 0);
}
const c = countLen(["ab", "cde"]);
console.log(c, typeof c, c.toFixed(1));

function sum(ns: number[]) {
  return ns.reduce((a, b) => a + b, 0);
}
console.log(sum([1, 2, 3]) * 2);

function sumNoSeed(ms: number[]) {
  return ms.reduce((a, b) => a + b);
}
console.log(sumNoSeed([4, 5]) + 1);

function flags(fs: number[]) {
  return fs.reduce((ok: boolean, x: number) => ok && x > 0, true);
}
const f = flags([1, 2]);
console.log(f, typeof f, !f);

function toStrs(ts: number[]) {
  const init: string[] = [];
  return ts.reduce((acc: string[], t: number) => acc.concat([String(t)]), init);
}
const strs = toStrs([1, 2]);
console.log(strs.join("-"), strs.length);

// an accumulator annotated with a union decides the result, never the
// seed's narrower type
function mixed(ms: number[]) {
  return ms.reduce((a: string | number, x: number) => (typeof a === "string" ? x : a + x), "");
}
const mx = mixed([1, 2, 3]);
console.log(mx, typeof mx, mixed([1, 2]) + 1);
