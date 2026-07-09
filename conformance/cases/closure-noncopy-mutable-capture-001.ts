// RFC 20260710 chunk 748 — a MUTATED non-Copy captured binding
// promotes to a capture box so every closure shares the live
// binding (ES §9.1) instead of an env-owns snapshot. Covers:
// top-level rebind visibility, fn-local, escaped closure,
// closure-side write, two closures sharing (writer + reader),
// array rebinding, and the never-written snapshot fast path.
let s1 = "a";
const f1 = () => s1;
s1 = s1 + "!";
console.log(f1());
function fnLocal(): string {
  let s = "a";
  const f = () => s;
  s = s + "!";
  return f();
}
console.log(fnLocal());
function mk(): () => string {
  let s = "m";
  const f = () => s;
  s = s + "?";
  return f;
}
console.log(mk()());
let s2 = "w";
const w2 = () => { s2 = s2 + "!"; };
w2();
console.log(s2);
function both(): string {
  let s = "a";
  const w = () => { s = s + "!"; };
  const r = () => s;
  w();
  w();
  return r() + "|" + s;
}
console.log(both());
let xs: number[] = [1];
const flen = () => xs.length;
xs = [1, 2, 3];
console.log(flen());
// never-written capture keeps the snapshot fast path
const k = "fixed";
const fk = () => k;
console.log(fk());
