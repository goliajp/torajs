// V3-18 m1.h.39 — `a % 0` on Number must yield NaN per JS spec
// §13.10 (Number.remainder). Pre-fix tora's i64 mod path used
// LLVM srem whose divisor-0 behavior is UB; in practice it
// silently returned 0.
//
// Fix: detect compile-time-zero divisor and emit ConstF64(NaN)
// directly. Runtime-zero divisor (`a % b` where b is loaded from
// a slot) is deferred — that needs branching IR + a result-type
// promotion, which changes the type contract and the bench
// surface area.

console.log(7 % 0)        // NaN
console.log(0 % 0)        // NaN
console.log(-5 % 0)       // NaN

// Spec-correct cases (no regression).
console.log(7 % 3)        // 1
console.log(-7 % 3)       // -1
console.log(7 % -3)       // 1
console.log(0 % 3)        // 0
console.log(10 % 5)       // 0
