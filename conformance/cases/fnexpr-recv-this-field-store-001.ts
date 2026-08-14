// `this.m = function () { … this … }` — a function expression stored
// into a field the class declares `any`. The store receivers this pass
// already admitted were all runtime-props shapes; the flattened
// `__this` was not one, so the stored function kept the enclosing
// CONSTRUCTOR's receiver and answered with it forever after. That is
// arrow semantics again: a function expression binds `this` at the
// call site (§10.2.1.2).
//
// An `any` slot is what carries the proof — the value comes back out
// as a NaN box, so every read of it enters the runtime any lane, where
// the call paths shift argv on the receiver flag. A slot typed with a
// concrete signature does not, and stays out.

class K {
  v = 5;
  f: any;
  constructor() { this.f = function () { return (this as any).v } }
}
const k = new K();
console.log(1, k.f.call({ v: 99 }));

// The field-initializer spelling is the same node once the class is
// flattened: the initializer runs as a constructor store.
class L {
  v = 5;
  g: any = function () { return (this as any).v };
}
console.log(2, new L().g.call({ v: 88 }));

// Read back as a method of its own holder, which is what the field
// spelling is usually for.
class N {
  v = 3;
  h: any;
  constructor() { this.h = function () { return (this as any).v } }
}
const n = new N();
console.log(3, n.h());

// No receiver at all — the strict answer, and the one the
// constructor's `this` used to mask.
class P {
  q: any;
  constructor() { this.q = function () { return (this as any) === undefined } }
}
const p = new P();
const detached = p.q;
console.log(4, detached());

// An arrow stored the same way keeps the lexical `this` (§8.3.4) — the
// half that must not move.
class A {
  v = 7;
  a: any;
  constructor() { this.a = () => (this as any).v }
}
console.log(5, new A().a.call({ v: 1 }));

// A stored function expression that never says `this` keeps the plain
// closure ABI: no receiver parameter, no argument shift.
class B {
  b: any;
  constructor() { this.b = function (x: number) { return x * 2 } }
}
console.log(6, new B().b(21));
