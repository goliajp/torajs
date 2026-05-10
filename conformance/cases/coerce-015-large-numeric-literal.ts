// V3-18 m1.h.19 — numeric literals beyond i64 range (e.g. 1e21,
// 1e22, 9.99e18) must lower to ConstF64, not be cast to i64
// (which saturates at i64::MAX = 9223372036854775807).
//
// Pre-fix: `1e21` printed as `9223372036854775807` because
// ssa_lower's classify checked only `n.fract() != 0.0`. Integer-
// valued doubles past i64 range fell through to `n as i64`.
//
// Now: classify → F64 if fractional OR magnitude ≥ 9.22e18 OR
// non-finite. Pairs with the f64-shortest print path so the
// output matches v8/JSC byte-for-byte: 1e21 → "1e+21",
// 9.99e18 → "9990000000000000000" (the actual f64
// representation).

console.log(1e21)
console.log(1e22)
console.log(1e23)
console.log(1e30)
console.log(9.99e18)
console.log(1e18)
console.log(1.5e3)

// Negative large.
console.log(-1e21)
console.log(-1e30)
