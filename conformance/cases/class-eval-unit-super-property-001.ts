// K5-3 — `super.x` / `super[k]` inside a per-evaluation family: the
// base is the [[Prototype]] of the home the running `this` stands
// under, per evaluation, at the declaring class's depth of the chain.
function mk(): any {
  class A {
    who() { return "A" }
    get g() { return "gA" }
    static tag() { return "sA" }
    base() { return typeof super.hasOwnProperty + ":" + String(super.extra) }
    static sbase() { return typeof super.call }
  }
  class B extends A {
    r() { return super.extra }
    k(n: string) { return super[n] }
    w() { return super.who() + "!" }
    g2() { return super.g }
    set(v: number) { super.own = v; return Object.keys(this).join() }
    viaArrow() { return [1].map(() => super.extra)[0] }
    static st() { return super.sextra }
    static sw() { return super.tag() + "!" }
    static sk(n: string) { return super[n] }
  }
  class C extends B {
    r() { return "C>" + super.r() }
    up() { return super.extra }
    static st() { return "C>" + super.st() }
  }
  return [A, B, C]
}
const [A1, B1, C1] = mk(), [A2, B2, C2] = mk();
A1.prototype.extra = 7;
A1.sextra = 8;
B1.prototype.extra = 70;
console.log(new B1().r(), new B2().r(), new B1().k("extra"), new B2().k("extra"), new B1().w(), new B1().g2());
console.log(new C1().r(), new C2().r(), new C1().up(), new C1().viaArrow(), new C1().base());
console.log(B1.st(), B2.st(), C1.st(), C2.st(), B1.sw(), B1.sk("sextra"), C1.sk("sextra"), A1.sbase(), C1.sbase());
console.log(new B1().set(5), A1 !== A2, new C1() instanceof A1, new C1() instanceof A2);
