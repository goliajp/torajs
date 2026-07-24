// K.3b follow-up (RFC 20260725 rotation 206) — an un-annotated
// all-literal ObjectLit top-level binding promotes to a module
// struct global under its synthesized __inlobj spelling: named fns
// see field reads, own-field writes, and whole reassignment.
let s = { a: 1, b: "x" };

function bump() {
  s.a = s.a + 10;
}
function readBoth(): string {
  return s.a + ":" + s.b;
}
console.log(readBoth());
bump();
console.log(readBoth());

// const form
const c = { n: 5, ok: true };
function flip() {
  c.ok = false;
  c.n = 6;
}
flip();
console.log(c.n + ":" + c.ok);

// f64-possible field: a named-fn write widens the slot's field
let w = { v: 1 };
function frac() {
  w.v = 2.5;
}
frac();
console.log(w.v);

// fractional literal field starts f64
let p = { x: 0.5 };
function half() {
  p.x = p.x / 2;
}
half();
console.log(p.x);

// whole-binding reassignment from a named fn
function swap() {
  s = { a: 7, b: "z" };
}
swap();
console.log(readBoth());

// main-scope reads/writes agree with named-fn views
s.a = 40;
console.log(s.a);
bump();
console.log(s.a);
