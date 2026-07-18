// rotation 140 — `x instanceof F` where F binds a class expression.
// test262 expressions/class/subclass-builtins/subclass-*Error* ×7:
// `const Sub = class extends Error {}; new Sub() instanceof Sub`
// returned false because the parser handed the raw binding name to
// Expr::InstanceOf while the lowering's descendant-tag set is keyed
// on real (synth) class names — empty set constant-folded to false.
// Fix: parse_comparison resolves the RHS ident through
// class_value_aliases (same map `new F()` / `F.method()` consume).

// 1) Anonymous class expression, no heritage.
const P = class {}
const p = new P()
console.log(p instanceof P)

// 2) Class expression extending a user class declaration.
class Base {}
const SubU = class extends Base {}
const su = new SubU()
console.log(su instanceof SubU, su instanceof Base)

// 3) Class expression extending a built-in error (the test262 shape).
const SubE = class extends Error {}
const se = new SubE()
console.log(se instanceof SubE, se instanceof Error)

// 4) Alias chain: instanceof through a propagated alias.
const F = class {}
const G = F
const g = new G()
console.log(g instanceof G, g instanceof F)

// 5) Negative: instance of one class expression is not an instance
//    of an unrelated one.
const A = class {}
const B = class {}
console.log(new A() instanceof B)
