// Primitive `.toString()` must resolve to the primitive method
// surface, not to a same-named user-class method.
//
// Pre-fix `cm_demote::is_builtin_container_ty` deliberately omitted
// Number / Boolean / BigInt on the "no false-reject evidence"
// assumption — so `desugar_classes`'s speculative rewrite of every
// `x.m(a)` into `__cm_<C>__m(x, a)` (single-owner mode) survived
// even when `x` later checked as a primitive. The checker's arg-admit
// gate then failed with `argument 0: expected ClassRef("X"), got Number`
// and the whole call was a compile error.
//
// Fix: add Number / Boolean / BigInt to the demotion table. Demoting
// sends the call through the primitive method surface (Number /
// String / Boolean / BigInt prototype interception in
// `ssa_lower_call_universal_methods`), which was already wired.
//
// String was already in the list so `"hi".toString()` was never broken;
// pin it here alongside the newly-fixed primitives so a future
// refactor can't silently regress it.

class X {
  a: number = 42
  toString(): string { return "X!" }
  valueOf(): number { return 99 }
  hasOwnProperty(_key: string): boolean { return true }
}

// The class shape stays reachable — its own instances still dispatch
// through `__cm_X__toString`.
console.log(new X().toString())                  // X!
console.log(new X().valueOf())                   // 99

// Primitives run their own prototype methods.
console.log((3).toString())                      // 3
console.log((10).toString(2))                    // 1010
console.log((255).toString(16))                  // ff
console.log((0).toString(36))                    // 0

console.log(true.toString())                     // true
console.log(false.toString())                    // false

console.log((10n).toString())                    // 10
console.log((255n).toString(16))                 // ff

console.log("hi".toString())                     // hi
console.log("hi".valueOf())                      // hi

// `.valueOf()` on Number / Boolean / BigInt returns the primitive
// itself per ES §21.1.3.30 / §20.3.3.3 / §21.2.3.4.
console.log((3).valueOf())                       // 3
console.log(true.valueOf())                      // true
console.log((10n).valueOf())                     // 10

// Sanity: builtin methods that were ALREADY demoted (map / set / etc.)
// stay demoted — the fix is additive.
const m = new Map<string, number>([["a", 1]])
console.log(m.get("a"))                          // 1

// Not-yet-covered residuals (kept as L3b context, no assert):
// - `(-42n).toString()` — receiver is `Expr::Unary { op: Neg, arg: BigInt }`,
//   not a bare literal, so the demote guard still rejects it.
// - `[1, 2, 3].toString()` — receiver is `Expr::Array`; same.
// Both would need the guard extended to cover unary/array-literal shapes
// (the concern the pre-fix guard called out: `type_of` on those shapes
// may carry affine bookkeeping side-effects beyond the pure-lookup
// assumption). Ship as a separate substrate wedge once a stress test
// pins the side-effect budget.
