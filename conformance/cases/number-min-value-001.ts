// V3-18 m1.h.38 — Number.MIN_VALUE per ECMA-262 §21.1.2.5 is
// the smallest positive Number value, which is the smallest
// *subnormal* double (5e-324), NOT the smallest *normal* double
// (f64::MIN_POSITIVE = 2.2250738585072014e-308).
//
// Pre-fix tora used Rust's f64::MIN_POSITIVE so:
//   Number.MIN_VALUE = 2.2250738585072014e-308   (wrong)
//   bun:               5e-324                    (spec)

console.log(Number.MIN_VALUE)             // 5e-324
console.log(Number.MAX_VALUE)             // 1.7976931348623157e+308
console.log(Number.MAX_SAFE_INTEGER)      // 9007199254740991
console.log(Number.MIN_SAFE_INTEGER)      // -9007199254740991
console.log(Number.EPSILON)               // 2.220446049250313e-16
console.log(Number.POSITIVE_INFINITY)     // Infinity
console.log(Number.NEGATIVE_INFINITY)     // -Infinity
console.log(Number.NaN)                   // NaN

// Sanity: MIN_VALUE / 2 underflows to 0.
console.log(Number.MIN_VALUE / 2)         // 0
console.log(Number.MIN_VALUE > 0)         // true
