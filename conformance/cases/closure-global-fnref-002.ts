// RFC 20260709-closure-global — named-fn reference init WITHOUT an
// explicit return annotation on the target (chunk 805). The forwarder
// wrap no longer requires the target's ret ann: desugar_implicit_generics
// backfills the forwarder's inferred ret after the wrap. Covers the
// inferred-number / void / f64 / object-ret / annotated / mutable-swap /
// shadow / reflection / main-only faces.

// inferred number ret
function take(x: number) { return x * 2 }
const f1 = take;
function g1() { return f1(21) }
console.log(g1());

// void target — canon spells void, call result is undefined
function logIt(m: string) { console.log(m) }
const f2 = logIt;
function g2() { f2("hi"); return 1 }
console.log(g2());
// NOTE: `console.log(f2("x"))` (void call as value) is a pre-existing
// silent-wrong across ALL lanes (direct named call answers garbage,
// closure lanes answer 0; bun: undefined) — tracked in L3b, not a
// face of this fixture.

// annotated binding, un-annotated target
const f3: (x: number) => number = take;
function g3() { return f3(21) }
console.log(g3());

// mutable swap inside a named fn body (assign-rhs wrap joins)
function inc(x: number) { return x + 1 }
function dec(x: number) { return x - 1 }
let op = inc;
function swap() { op = dec }
function apply(n: number) { return op(n) }
console.log(apply(10));
swap();
console.log(apply(10));

// f64 float ret inference
function half(x: number) { return x / 2 }
const f4 = half;
function g4() { return f4(21) }
console.log(g4());

// object-ret inference
function mk(n: number) { if (n > 0) { return { v: n } } return { v: 0 } }
const f5 = mk;
function g5() { return f5(3).v + f5(-1).v }
console.log(g5());

// fn-local shadow keeps localizing
const f6 = take;
function g6() { let f6 = 7; return f6 }
function h6() { return f6(3) }
console.log(g6(), h6());

// reflection
console.log(typeof f1, f1.name);

// main-only binding keeps the direct-dispatch home
const f7 = take;
console.log(f7(5));

// captured by a closure AND read by a named fn
function greet(s: string) { return "hi " + s }
const f8 = greet;
const use8 = () => f8("cap");
function g8() { return f8("fn") }
console.log(use8(), g8());
