// RFC 20260719-ns-static-value-reify B1 — namespace-static builtin
// methods read as VALUES: interned dispatcher cells (identity),
// alias calls through the boxed lane, reflection faces, any lane.
const m = Math.max;
console.log(m(1, 5));
console.log(Math.max);
console.log(m);
console.log(typeof m);
console.log(Math.max === Math.max);
console.log(m === Math.max);
console.log(m.name);
console.log(m.length);
console.log(m.toString());
console.log(m.call(null, 3, 9));
console.log(String(m));

// any lane — call, typeof, reflection, .call re-dispatch
const a: any = Math.abs;
console.log(a(-7));
console.log(typeof a);
console.log(a.name, a.length);
console.log(a.call(null, -5));

// per-family dispatch shapes
const s = Math.sqrt;
console.log(s(16));
const fl = Math.floor;
console.log(fl(3.7));
const t = Math.trunc;
console.log(t(-3.9));
const p = Math.pow;
console.log(p(2, 8));
const im = Math.imul;
console.log(im(3, 4));
const cz = Math.clz32;
console.log(cz(1));
const mn = Math.min;
console.log(mn(3, -2));

// random through the value face — range-only assertion
const r = Math.random;
const x = r();
console.log(x >= 0 && x < 1);

// a user binding shadowing Math never routes the mint gate
function shadowed(): number {
  const Math = { max: (u: number, v: number) => u - v };
  return Math.max(10, 3);
}
console.log(shadowed());
