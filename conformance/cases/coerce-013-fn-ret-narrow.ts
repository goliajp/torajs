// V3-18 m1.h.16 — when fn ret is declared `: number` (i64 in
// tora's strict-typed model) but the body returns an f64 value
// (e.g. Math.abs / Math.floor / Math.ceil / Math.round all
// return f64 per JS spec), the lowerer must narrow f64 → i64
// at the return point. Symmetric to the existing i64 → f64
// promotion at the same site.
//
// Pre-fix: LLVM verify rejected `ret double %X` from a fn with
// `i64` return type, hard-erroring at compile time.
//
// Now: FpToSi truncates the fractional part (same as `| 0` would).

function f(x: number): number { return Math.abs(x) }
console.log(f(-7))
console.log(f(5))

function g(x: number): number { return Math.floor(x) }
console.log(g(3))
console.log(g(7))

function h(x: number): number { return Math.max(x, 10) }
console.log(h(3))
console.log(h(20))
