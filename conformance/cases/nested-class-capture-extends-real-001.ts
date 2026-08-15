// A capturing subclass extends a top-level REAL class (405-01 face
// 2): ctor inheritance rides the __ctorany_<P> twin (field writes
// through the any lane onto the dynobj instance), instance methods
// ride Object.create(P.prototype) + the twin-guarded chain dispatch,
// statics ride the function value's user [[Prototype]] chain into
// the class object, and instanceof walks the chain.
class P {
  x: number
  tag = "p"
  constructor(x: number) { this.x = x }
  m() { return this.x + 1 }
  static s() { return 20 }
}
function mk(v: number) {
  class C extends P {
    g() { return v }
  }
  return new C(v)
}
const c: any = mk(7)
console.log(c.x, c.tag, c.m(), c.g(), c instanceof P)
function mk2(v: number) {
  class D extends P {
    y: number
    constructor(a: number) { super(a * 2); this.y = v + a }
    h() { return this.x + this.y }
  }
  return new D(3)
}
const d: any = mk2(10)
console.log(d.x, d.y, d.h(), d instanceof P)
function mk3(v: number) {
  class E extends P { e() { return v } }
  class F extends E { f() { return this.e() + this.x } }
  return new F(5)
}
const f: any = mk3(100)
console.log(f.x, f.e(), f.f(), f instanceof P)
function mk4(v: number) {
  class G extends P { static u() { return v } }
  return G
}
const G: any = mk4(9)
console.log(G.s(), G.u())
