// §13.3.6 — `super[k](…)` invokes with the CURRENT `this` as
// receiver, off a base re-read at every access.
class B {
  t() { return "B.t"; }
  who() { return this.constructor.name; }
}
class C extends B {
  t3() { return super["t"](); }
  w() { return super["who"](); }
}
const c = new C();
console.log(c.t3());
console.log(c.w());

// GetSuperBase is not cached: a later prototype write is visible.
class P { m() { return "P.m"; } }
class Q extends P { go() { return super["m"](); } }
const q = new Q();
console.log(q.go());
(P.prototype as any).m = function () { return "patched"; };
console.log(q.go());

// Args, including a spread element.
class A2 { sum(a: number, b: number, c: number) { return a + b + c; } }
class B2 extends A2 { s() { const xs = [2, 3]; return super["sum"](1, ...xs); } }
console.log(new B2().s());

// A numeric key ToPropertyKey-s to "1".
class N1 { 1() { return 11; } }
class N2 extends N1 { go() { return super[1](); } }
console.log(new N2().go());
