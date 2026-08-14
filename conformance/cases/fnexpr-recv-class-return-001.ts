// A function expression RETURNED from a class member body binds its
// `this` at the call site (§10.2.1.2). The object-literal spelling of
// that position has promoted its receiver since rotation 346; the
// class spelling could not be seen at all, because `desugar_classes`
// flattens each member into a top-level FnDecl and the collector
// recognized its methods by the lifted closure names the
// object-literal pass publishes. So the returned function was handed
// the METHOD's receiver — arrow semantics, right only when the two
// receivers happen to be the same object.
//
// The promote needs the value to cross into the any lane and stay
// there (every any-lane call path shifts argv on the receiver flag; a
// typed indirect call does not), so it reads the member's return
// annotation, which has to be spelled exactly `any`: leaving it off
// infers the return type FROM the returned function expression, which
// hands the caller a typed callee whose call lane does not shift argv.
// That half keeps today's answer.

class M {
  v = 1;
  make(): any { return function () { return (this as any).v } }
  probe(): any { return function () { return (this as any) === undefined } }
}
const m = new M();
console.log(1, m.make().call({ v: 42 }));

// Called with no receiver at all — the strict answer, and the one the
// enclosing method's `v` would have masked.
console.log(2, m.probe()());

// A static member is the same position on the other side of the class.
class S {
  static v = 7;
  static make(): any { return function () { return (this as any).u } }
}
console.log(3, S.make().call({ u: 5 }));

// The object-literal spelling next door, pinned so the two read as one
// answer rather than two.
const o = { v: 2, make(): any { return function () { return (this as any).v } } };
console.log(4, o.make().call({ v: 9 }));

// An ARROW returned from the same slot keeps the lexical `this`
// (§8.3.4) — the half that must not move.
class A {
  v = 6;
  make(): any { return () => (this as any).v }
}
console.log(5, new A().make()());

// A returned function expression that never says `this` keeps the
// plain closure ABI: no receiver parameter, no argument shift.
class P {
  make(): any { return function (n: number) { return n + 1 } }
}
console.log(6, new P().make()(10));
