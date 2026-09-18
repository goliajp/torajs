// §13.3.7.3 — `super.m()` in a static body runs `m` with the CURRENT
// `this`: the class the call went through, not the class declaring `m`.
// A `super.create()` factory must build the subclass.
class A { static s() { return this } static who() { return this.name } static plain() { return 1 } }
class B extends A { static t() { return super.s() } static w() { return super.who() } static p() { return super.plain() + 1 } }
class C extends B {}
console.log(B.t() === B, B.s() === B);
console.log(C.t() === C, B.w(), C.w());
console.log(B.p(), C.p());
const X = { name: "x" };
console.log(B.t.call(X) === X);
class F { v = 0; static create() { const o: any = new this(); o.v = 1; return o } }
class G extends F { static create() { const o: any = super.create(); o.v += 10; return o } }
class H extends G {}
const g: any = G.create(), h: any = H.create();
console.log(g instanceof G, g.v, h instanceof H, h.v);
