// Container-element / field / fn-ret width faces (ann-width W4 family,
// recorded 2026-06-11, verified fixed 2026-07-22): a fractional write
// into a number[] slot, a struct field, and a class field must read
// back as the written value — not bit-punned I64 garbage.
let a: number[] = [1, 2];
a[0] = 0.5;
console.log(a[0]);
let b = [1.5, 2.5];
b[0] = 3;
console.log(b[0], b[1]);
type P = { v: number };
let p: P = { v: 1 };
p.v = 0.5;
console.log(p.v);
class C {
  w: number;
  constructor() {
    this.w = 1;
  }
}
let c = new C();
c.w = 0.25;
console.log(c.w);
function f(): number {
  return 0.75;
}
let xs: number[] = [1];
xs[0] = f();
console.log(xs[0]);
