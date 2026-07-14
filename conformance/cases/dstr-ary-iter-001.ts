// RFC 20260714-dstr-residual blade 3 — array destructuring reads its
// source through the iterator protocol (ES §13.15.5.3) whenever that
// source is not a statically indexable container.
//
// The parse-time desugar indexes (`src[0]`, `src[1]`, `src.slice(N)`),
// which is right for an Array and wrong for everything else. A
// generator has no index; before this, a typed one was rejected outright
// (`no member .0 on Struct([__gen_nominal_g, …])`) and one behind `any`
// silently bound `undefined` to every name.

function* g() {
  yield 1;
  yield 2;
  yield 3;
}

// Typed generator source.
const [a, b] = g();
console.log(a, b);

// The same generator behind `any` — the silent-undefined case.
const src: any = g();
const [c, d] = src;
console.log(c, d);

// A Set: iterable, not indexable (`can't index into Set` before).
const s = new Set([7, 8, 9]);
const [e, f] = s;
console.log(e, f);

// An Array source keeps the index reads — same lane as before.
const pair: number[] = [10, 20];
const [p, q] = pair;
console.log(p + q);

// Rest drains the tail.
const [h, ...rest] = g();
console.log(h, rest.length, rest[0], rest[1]);

// Defaults fire on a short source; elisions still step the iterator.
const [x = 9, y = 8, z = 7, w = 6] = g();
console.log(x, y, z, w);
const [, second] = g();
console.log(second);

// Param position — the pattern reads a synthesized param the same way.
function fromParam([m, n]: any) {
  console.log("param", m, n);
}
fromParam(g());

const arrow = ([i, j]: any) => console.log("arrow", i, j);
arrow(g());
