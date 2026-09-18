// Static `super.m()` receiver, the other positions: an owner further up
// the chain, a static field initializer and a static block (`this` is
// the class there), a typed return, and a chain of super calls.
class A { static id() { return this } }
class M extends A {}
class D extends M {
  static v() { return super.id() }
  static f = super.id() === this;
  static { console.log("blk", super.id() === this) }
}
class E extends D {}
console.log(D.v() === D, E.v() === E, (D as any).f);
class P { static n(): number { return 1 } static me() { return this } }
class Q extends P { static n(): number { return super.n() + 1 } static chain() { return super.me() } }
class R extends Q { static chain() { return super.chain() } }
console.log(Q.n(), R.chain() === R, Q.chain() === Q);
