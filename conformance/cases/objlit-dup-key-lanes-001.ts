// r503 — a literal's static keys store through the insert-only fresh
// kernel when the lowering can prove them fresh; a repeated key, and
// every key after a computed key or a spread, keeps the duplicate-
// capable kernel (last write wins, the first value dropped). Each
// shape below lands a heap value on a key that already holds one.
const dup = { a: [1, 2], b: "x", a: [3] };
console.log(dup);
const k = "a";
const afterComputed = { [k]: { v: 1 }, a: { v: 2 }, c: 3 };
console.log(afterComputed);
const src = { a: "from-src", d: [9] };
const afterSpread = { ...src, a: "static", d: [10, 11] };
console.log(afterSpread);
const nested = { p: { q: 1 }, p: { q: 2 } };
console.log(nested.p.q);
let acc = 0;
for (let i = 0; i < 300; i++) {
  const o = { s: "one" + i, s: "two" + i, n: i, n: i * 2 };
  acc += o.n + o.s.length;
}
console.log(acc);
