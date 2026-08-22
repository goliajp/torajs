// §13.3.7 — `super[k]` reads with the CURRENT `this` as receiver, so
// an accessor on the base runs against `this`, not against the base.
let viaCall: any;
let viaMember: any;
class Parent {
  getThis() { return this; }
  get This(): any { return this; }
}
class C extends Parent {
  method() { viaCall = super["getThis"](); viaMember = super["This"]; }
}
(C.prototype as any).method();
console.log(viaCall === C.prototype, viaMember === C.prototype);

// An accessor defined at RUN time is unreachable to any static walk;
// the computed read still carries the receiver.
class A1 {}
class A2 extends A1 { tag = "a2"; go() { return super["dyn"]; } }
Object.defineProperty(A1.prototype, "dyn", { get(this: any) { return this.tag; } });
console.log(new A2().go());

// A plain data property answers itself; a missing one answers
// undefined rather than throwing.
class D1 { d = 1; }
(D1.prototype as any).p = "proto-p";
class D2 extends D1 { go() { return [super["p"], super["nope"]]; } }
console.log(JSON.stringify(new D2().go()));

// The key expression is evaluated exactly once.
let n = 0;
class K1 { k0 = 0; }
class K2 extends K1 { go() { return [super["k" + (n++)], n]; } }
console.log(JSON.stringify(new K2().go()));
