// A TS type assertion emits no code, so it cannot change whether the
// operand of `delete` is a Reference — `delete (x as any)` is
// `delete x`. The parser's operand triage saw the `As` node instead
// and routed it to the not-a-Reference lane, whose answer is a
// constant `true`. All three answers were wrong at once: a declared
// binding said true where §9.1.1.1.7 makes it non-configurable, an
// undeclared name died at COMPILE time on an unknown identifier, and
// the strict-goal early error (§13.5.1.1) never fired.
//
// Sloppy goal (`.cts`), because a bare `delete` on a name is a
// SyntaxError under strict and that half cannot be bun-compared: tr
// and bun both refuse it, with different text.

var v = 1;
let l = 2;
function fn(): void {}
class C {}

// Declared bindings are non-configurable: false, and still there.
console.log(delete (v as any), typeof v);
console.log(delete (l as any), typeof l);
console.log(delete (fn as any), typeof fn);
console.log(delete (C as any), typeof C);

// Assertions nest.
console.log(delete (v as any as number as any), typeof v);

// An unresolvable name answers true (§13.5.1.2 step 3.a) — and must
// not be a compile-time error.
console.log(delete (nosuchbinding as any));

// A parameter is a binding too.
function withParam(p: number): void {
  console.log(delete (p as any), typeof p);
}
withParam(7);

// Through an assertion, a PROPERTY reference is still the real
// delete, not the constant lane.
var o: any = { k: 1, j: 2 };
console.log(delete (o.k as any), o.k, "j" in o);
console.log(delete ((o as any).j as any), o.j);

// Controls: the bare and parenthesized forms were already right.
var b = 3;
console.log(delete b, typeof b);
console.log(delete (b), typeof b);
console.log(delete nosuchbinding2);
