// V3-18 wedge — Number.prototype.toFixed half-rounding spec
// compliance. Per JS spec §21.1.3.4, ties are broken by
// choosing the larger m — i.e. round half AWAY from zero.
//   (1.5).toFixed(0)    → "2"   ✓ already
//   (2.5).toFixed(0)    → "3"   (spec) but pre-fix tora gave "2"
//   (1234.5).toFixed(0) → "1235"               pre-fix gave "1234"
// macOS libc's snprintf(%.*f) defaults to round-half-to-even
// (banker's rounding) which diverges at .5 boundaries.
//
// Fix: at the runtime helper, pre-multiply by 10^digits,
// round() (half-away-from-zero per C99 §7.12.9.6), divide
// back, then format. Guarded behind digits<16 to keep
// large-precision cases on the snprintf path (avoids f64
// precision loss at small magnitudes from the multiply).

// Tie-break cases (pre-fix divergences).
console.log((2.5).toFixed(0))          // 3
console.log((1234.5).toFixed(0))       // 1235
console.log((-2.5).toFixed(0))         // -3
console.log((-0.5).toFixed(0))         // -1

// Already-correct half-up cases.
console.log((1.5).toFixed(0))          // 2
console.log((1.6).toFixed(0))          // 2

// Non-half cases.
console.log((1234.4).toFixed(0))       // 1234
console.log((1234.6).toFixed(0))       // 1235

// digits > 0 — common decimal-place rounding.
console.log((3.14159).toFixed(2))      // 3.14
console.log((3.14159).toFixed(4))      // 3.1416

// Integer receiver passes through with trailing zeros.
let n = 42
console.log(n.toFixed(2))              // 42.00
console.log(n.toFixed(0))              // 42

// Edge: zero / 0-digits.
console.log((0).toFixed(0))            // 0
console.log((0).toFixed(5))            // 0.00000
