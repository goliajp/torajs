// Two classes that share a NAME and carry a computed-key static field.
// The side tables that hold the computed keys are keyed by class name,
// so the second declaration used to overwrite the first: the field went
// missing, a plain sibling static field on the same-named class went
// missing with it, and a top-level class could read the local class's
// value.
const k = "kk", k2 = "mm";
function a1() { class A { static [k] = 1; } return (A as any)[k]; }
function a2() { class A { static plain = 2; } return (A as any).plain; }
console.log("a", a1(), a2());
function d1() { class E { static [k] = 31; } return (E as any)[k]; }
function d2() { class E { static [k] = 32; } return (E as any)[k]; }
function d3() { class E { static [k] = 33; } return (E as any)[k]; }
console.log("d", d1(), d2(), d3());
class F { static [k] = 41; }
function e2() { class F { static [k] = 42; } return (F as any)[k]; }
console.log("e", (F as any)[k], e2());
function f1() { class G { static [k] = 51; static [k2] = 52; } return [(G as any)[k], (G as any)[k2]].join("/"); }
function f2() { class G { static [k] = 53; static [k2] = 54; } return [(G as any)[k], (G as any)[k2]].join("/"); }
console.log("f", f1(), f2());
// the capturing lane: the class body also reads a binding from around it
function g1() { let n = 0; class H { static [k] = ++n; m() { return "g1" + n; } } return new H().m() + ":" + (H as any)[k]; }
function g2() { let n = 10; class H { static [k] = ++n; m() { return "g2" + n; } } return new H().m() + ":" + (H as any)[k]; }
console.log("g", g1(), g2());
// shapes that were already right, kept as the control row
function b1() { class B { [k] = 11; } return (new B() as any)[k]; }
function b2() { class B { [k] = 12; } return (new B() as any)[k]; }
function c1() { class C { [k]() { return 21; } } return (new C() as any)[k](); }
function c2() { class C { [k]() { return 22; } } return (new C() as any)[k](); }
console.log("ctl", b1(), b2(), c1(), c2());
