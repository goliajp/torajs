// P3.struct-method-dispatch — `obj.method()` on inline struct (non-
// class, Type::Obj). Pre-fix tora silently returned garbage int (a
// pointer-cast leak) instead of invoking the method; even more
// dangerous than a panic because user code sees a plausible-looking
// number and may use it downstream. L4 trigger #1 for closing P3.
//
// Scope: FnSig-typed struct fields (top-level fn assigned or
// non-capturing function expression that lift_arrow_fns lifts to
// FnSig). Closure-typed struct fields (with capture) are the next
// item — separate L3a step.

// 0-arg method, primitive return.
type N0 = { v: () => number };
const n0: N0 = { v: function(): number { return 42; } };
console.log(n0.v());  // 42

// 0-arg method, string return.
type S0 = { s: () => string };
const s0: S0 = { s: function(): string { return "hello"; } };
console.log(s0.s());  // hello

// 1-arg method.
type N1 = { f: (x: number) => number };
const n1: N1 = { f: function(x: number): number { return x + 1; } };
console.log(n1.f(10));  // 11

// 2-arg method.
type N2 = { g: (a: number, b: number) => number };
const n2: N2 = { g: function(a: number, b: number): number { return a * b; } };
console.log(n2.g(6, 7));  // 42

// Method that references a non-captured top-level helper.
function helper(): number { return 100; }
type H = { call_helper: () => number };
const h: H = { call_helper: function(): number { return helper() + 1; } };
console.log(h.call_helper());  // 101

// Sequential calls — same struct, repeated invoke.
type C = { c: () => number };
const c: C = { c: function(): number { return 7; } };
console.log(c.c() + c.c() + c.c());  // 21

// Method-call result in arithmetic expression.
console.log(n2.g(2, 3) * 10);  // 60
