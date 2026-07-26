// RFC 20260726-new-on-function blade 2 — `new F()` on a function.
// desugar_classes rewrites every `new X()` into `__new_X(...)` without
// checking X is a class, and only classes ever got that factory.

function Con(x: number) { this.x = x; }
const c = new Con(1);
console.log(c.x);

function Point(x: number, y: number) { this.x = x; this.y = y; }
const p = new Point(3, 4);
console.log(p.x, p.y);

// Instances are independent objects, not one shared blob.
const p2 = new Point(10, 20);
console.log(p.x, p.y, p2.x, p2.y);

// The receiver is dynamic, so a field may be assigned on one branch
// only — the thing a nominal layout could not express.
function Flexible(n: number) {
  this.n = n;
  if (n > 5) { this.big = true; }
}
const small = new Flexible(1);
const big = new Flexible(9);
console.log(small.n, big.n, big.big);

// A function that never mentions `this` is still constructible; it just
// never receives the instance.
function Empty() {}
const e = new Empty();
console.log(typeof e);

// Fields added after construction.
const q = new Point(1, 2);
q.z = 99;
console.log(q.x, q.y, q.z);

// Classes still take the class path — the two factories coexist.
class K {
  v: number = 5;
  bump(): number { return this.v + 1; }
}
const k = new K();
console.log(k.v, k.bump());

// A constructor calling a plain function.
function double(v: number): number { return v * 2; }
function Scaled(v: number) { this.v = double(v); }
console.log(new Scaled(21).v);
