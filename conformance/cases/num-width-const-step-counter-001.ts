// rotation 507 (506-06) — a counter step written `const step = 3` is the
// same small-step counter as one written `3`: without the const-int
// table the accumulator was marked growth, kept an f64 slot, and paid a
// versioned loop plus a per-iteration guard the literal form never
// sees. The magnitude rule applies to both spellings alike, so a big
// const step still marks growth and rounds like bun. Shapes: small
// const step, negative const step, a big const step, a MUTABLE binding
// reassigned to a big value mid-loop (never eligible), a fn-local const
// shadowing a top-level one of the same name, and a const step read
// inside a named fn.
const step = 3;
const back = -7;
const big = 9007199254740991;
let u = 0;
for (let i = 0; i < 100000; i++) u += step;
console.log(u);
let d = 0;
for (let i = 0; i < 100000; i++) d += back;
console.log(d);
let t = 0;
for (let i = 0; i < 2000; i++) t += big;
console.log(t);
let m = 3;
let w = 0;
for (let i = 0; i < 10; i++) {
  w += m;
  m = 9007199254740991;
}
console.log(w);
function shadowed(): number {
  const step = 9007199254740991;
  let a = 0;
  for (let i = 0; i < 5; i++) a += step;
  return a;
}
console.log(shadowed());
function viaConst(n: number): number {
  let a = 0;
  for (let i = 0; i < n; i++) a += step;
  return a;
}
console.log(viaConst(1000), viaConst(3));
let s = 0;
for (let i = 0; i < 50000; i++) s = s + step;
console.log(s);
