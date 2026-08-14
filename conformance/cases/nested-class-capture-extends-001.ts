// blade 5 — a captured nested class extending a captured sibling
// lowers to the ES5 constructor pattern (RFC 20260814).
{
  let a = 7;
  class P {
    v: number;
    constructor(p) { this.v = p + a }
    m() { return this.v * 10 }
    g() { return 20 }
  }
  class D extends P {
    w: number;
    constructor(p) { super(p); this.w = 1 }
    m() { return super.m() + this.w }
  }
  const d = new D(2);
  console.log(d.m(), d.g(), d instanceof P, d instanceof D, d.constructor === D);
}
// implicit forwarding ctor + grandparent chain
{
  let b = 1;
  class A { p: number; constructor(x, y) { this.p = x + y + b } }
  class B extends A { q: number; constructor() { super(2, 3); this.q = 10 } }
  class C extends B {}
  const c = new C();
  console.log(c.p, c.q, c instanceof A, c instanceof B, c instanceof C);
}
// fresh identity per factory call
function mk(n) {
  class P2 { v: number; constructor() { this.v = n } }
  class D2 extends P2 { sum() { return this.v + n } }
  return new D2();
}
console.log(mk(1).sum(), mk(10).sum());
// super argument evaluation order rides through the lowering
{
  let z = 1;
  let log: string[] = [];
  function t(s) { log.push(s); return s.length }
  class P3 { s: number; constructor(x, y) { this.s = x * 100 + y + z } }
  class D3 extends P3 { constructor() { super(t("ab"), t("xyz")) } }
  console.log(new D3().s, log.join(","));
}
