// A function with no return annotation that hides a value of another
// kind behind `any` hands it on to a function declared `number` that
// reads a member of its answer or takes it as an operand: the member
// or the operation answers what the hidden value gives, and the
// declared return hands that back as is.
function g() { return "s" as any; }
function h() { return g() + 1; }
function f(): number { return h(); }
function f2(): number { return g() + 1; }
function f3(): number { const t = g(); return t + 1; }
function o() { return { n: "x" } as any; }
function f4(): number { return o().n; }
function a() { return ["y"] as any; }
function f5(): number { return a()[0]; }
function m() { return { v: g() }; }
function f6(): number { return m().v; }
function k() { return 2 as any; }
function f7(): number { return k() * 3; }
console.log(f(), f2(), f3(), f4(), f5(), f6(), f7());
