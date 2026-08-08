// SuperProperty read (§13.3.7): runtime prototype data properties with
// shadowing — super base starts at the PARENT's prototype, so the
// subclass's own shadowing entries are skipped (t262 prop-dot-cls-val
// core shape).
var fromA = "", fromB = "";
class A {}
class B extends A {}
class C extends B {
  method() {
    fromA = super.fromA;
    fromB = super.fromB;
  }
}
(A.prototype as any).fromA = "a";
(A.prototype as any).fromB = "a";
(B.prototype as any).fromB = "b";
(C.prototype as any).fromA = "c";
(C.prototype as any).fromB = "c";
new C().method();
console.log(fromA, fromB);

// Statically-declared getter on the chain: dispatched with the
// CURRENT `this` as receiver (§13.3.7 MakeSuperPropertyReference),
// both directly and through an arrow (arrows share the method's
// super binding).
class P {
  v = 10;
  get g(): number {
    return this.v * 2;
  }
}
class Q extends P {
  direct(): number {
    return super.g;
  }
  viaArrow(): number {
    return (() => super.g)();
  }
}
const q = new Q();
q.v = 21;
console.log(q.direct(), q.viaArrow());
