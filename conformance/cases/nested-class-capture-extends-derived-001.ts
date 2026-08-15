// 405-01 residue — a capturing class may extend a DERIVED top-level
// real class: the hoist admits any top-level real class whose whole
// extends chain is made of such classes (was: root only), the claim
// closes es5_real_parents over the chain, and a derived parent's
// ctor twin retargets its pass-1.5 super(…) rewrite at the parent's
// own __ctorany_ twin (resolved through es5_ctor_forward, so a
// silent ctor-less link is skipped).

class Base { v = 1; constructor(x: number) { this.v = x } bm() { return "B" } }
class Mid extends Base { w = 2 }

// ctor-less middle link — its folded field-init ctor is the landing
function make() {
  let k = 10
  class Sub extends Mid { m() { return this.v + this.w + k } }
  return new Sub(5)
}
const s: any = make()
console.log(s.v, s.w, s.m())
console.log(s instanceof Mid, s instanceof Base)
console.log(s.bm())

// explicit middle ctor with its own super(…)
class M2 extends Base { u: number; constructor(x: number) { super(x * 2); this.u = x } }
function make2() {
  let t = 100
  class S2 extends M2 { n() { return this.v + this.u + t } }
  return new S2(3)
}
const s2: any = make2()
console.log(s2.v, s2.u, s2.n())

// root-parent capturing form must not regress
function make3() {
  let z = 7
  class S3 extends Base { p() { return this.v + z } }
  return new S3(2)
}
console.log((make3() as any).p())
