// a named function in a choice with `null` / `undefined`, handed to
// an `any` slot or kept in a binding: the function value is its fn
// object, and a `null` arm written first keeps the callable's repr
function inc(n: number): number { return n + 1; }
function dbl(n: number): number { return n * 2; }
function boxed(c: boolean) {
  const g: any = c ? null : inc;
  console.log(typeof g, g === null ? "null" : g(1), g === inc);
}
boxed(false);
boxed(true);
function nested(c: boolean, d: boolean) {
  const m: any = d ? inc : c ? dbl : null;
  return typeof m + " " + (m === null ? "-" : m(5));
}
console.log(nested(false, true), nested(true, false), nested(false, false));
function inferred(c: boolean) {
  const h = c ? null : inc;
  if (h) return h(2);
  return "none";
}
console.log(inferred(false), inferred(true));
function annotated(d: boolean) {
  const k: ((n: number) => number) | null = d ? dbl : null;
  return k === null ? "null" : k(3);
}
console.log(annotated(false), annotated(true));
function relayed(c: boolean) {
  const h = c ? null : dbl;
  const g: any = h;
  return typeof g;
}
console.log(relayed(false), relayed(true));
function undef(c: boolean) {
  const u: any = c ? undefined : dbl;
  return typeof u;
}
console.log(undef(false), undef(true));
let t = 0;
for (let i = 0; i < 200000; i++) {
  const g: any = i % 3 === 0 ? null : inc;
  if (g !== null) t += g(i);
}
console.log(t);
