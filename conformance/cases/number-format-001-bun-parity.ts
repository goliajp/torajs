// P3.2-c.3.b — Number.prototype.toFixed / toExponential / toPrecision
// bun-parity coverage beyond number-tofixed-001/-002. Targets the new
// torajs-num::format port (Rust replacement for the old runtime_str.c
// __torajs_num_to_{fixed,exp,precision}_{i,f} family).
//
// Wedges intentionally NOT exercised here (preserve C subset bit-for-
// bit; see torajs-num::format module docs for the L3b cleanup list):
//   - special values: tr emits "nan"/"inf"/"-inf" (snprintf %f legacy),
//     bun emits "NaN"/"Infinity"/"-Infinity" (JS spec).
//   - toPrecision trailing-zero strip: tr uses C %g strip semantics
//     ((1.5).toPrecision(3) → "1.5"), bun keeps zeros per JS spec
//     ((1.5).toPrecision(3) → "1.50").
//   - toExponential / toPrecision rounding ties: tr/C use libc / Rust
//     half-even, bun uses JS spec half-away-from-zero.

// --- toExponential ---
console.log((100).toExponential(0))         // 1e+2
console.log((100).toExponential(2))         // 1.00e+2
console.log((0).toExponential(3))           // 0.000e+0
console.log((0.001).toExponential(2))       // 1.00e-3
console.log((0.00001234).toExponential(4))  // 1.2340e-5
console.log((-100).toExponential(2))        // -1.00e+2
console.log((1e21).toExponential(5))        // 1.00000e+21

// --- toPrecision (no trailing-zero cases) ---
console.log((123.456).toPrecision(4))       // 123.5
console.log((123.456).toPrecision(6))       // 123.456
console.log((1234567).toPrecision(3))       // 1.23e+6
console.log((0.0001234).toPrecision(3))     // 0.000123
console.log((12.5).toPrecision(3))          // 12.5
console.log((-0.5).toPrecision(1))          // -0.5

// --- toFixed (beyond -001/-002 — pre-multiply round-half-away-from-zero) ---
console.log((9.99).toFixed(1))              // 10.0
console.log((-9.99).toFixed(1))             // -10.0
console.log((0).toFixed(20))                // 0.00000000000000000000
console.log((100).toFixed(0))               // 100
console.log((100).toFixed(2))               // 100.00
console.log((-7).toFixed(3))                // -7.000

// --- Typed-number receiver dispatch ---
const x: number = 42
console.log(x.toExponential(2))             // 4.20e+1
console.log(x.toFixed(3))                   // 42.000
