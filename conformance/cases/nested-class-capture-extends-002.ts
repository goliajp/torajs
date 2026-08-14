// blade 5 × member faces — accessor, computed name, own statics, and
// this-reading static init all land on a subclass of a captured
// sibling (RFC 20260814).
{
  let a = 3;
  const k = "dyn";
  class P { v: number; constructor() { this.v = a } }
  class D extends P {
    get w() { return this.v + 100 }
    [k](n) { return this.v * n }
    static mk() { return a * 2 }
  }
  const d = new D();
  console.log(d.w, d.dyn(5), D.mk());
}
{
  let a2 = 2;
  class P4 { q: number; constructor() { this.q = a2 } }
  class D4 extends P4 {
    static base = a2 * 10;
    static twice = this.base * 2;
    static { this.tag = this.base + 1 }
  }
  console.log(D4.base, D4.twice, D4.tag, new D4().q);
}
