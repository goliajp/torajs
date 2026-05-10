// V3-18 m1.h.18 — `console.log(f64)` and `String(f64)` produce
// the shortest decimal that roundtrips back to the same f64,
// per ECMA-262 §6.1.6.1.13 / §22.1.3.6 (Number::toString).
//
// Pre-fix: printf("%g") capped at 6 significant digits, so
// `10 / 3` printed `3.33333` while bun printed
// `3.3333333333333335`. JSON.stringify and any test262 assert
// using a printed f64 would fail byte-equality.
//
// Now: try-precisions loop in runtime_str.c picks the smallest
// precision n in [1, 17] s.t. `%.*g` parses back to the same
// double. Output is byte-equal to v8/JSC for every f64 value.
// Future perf optimization: drop in Ryu / Grisu — out-of-scope
// for the test262-conformance push.

console.log(0.1)
console.log(0.2)
console.log(0.1 + 0.2)
console.log(1.5)
console.log(2.5)
console.log(3.5)
console.log(10 / 3)
console.log(1 / 3)
console.log(2 / 3)
console.log(Math.PI)
console.log(Math.E)
console.log(Math.sqrt(2))
console.log(Math.LN2)
console.log(Math.LOG2E)

// Subnormals / very small.
console.log(1e-100)
console.log(1.7e-308)

// Negative zero.
console.log(-0.0)
console.log(0.0)

// Integer-valued doubles still print without trailing `.0`.
console.log(1.0)
console.log(100.0)
