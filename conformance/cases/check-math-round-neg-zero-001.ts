// ES §20.2.2.28 — `Math.round(x)` for x in (-0.5, 0) returns -0, not +0.
// The textbook `floor(x + 0.5)` implementation gives 0 for these inputs
// (the sign bit drops in the addition); the spec requires preserving
// the input's sign. Pre-fix tr matched the canonical algorithm; bun
// matches the spec exception. Fix adds a narrow guard at the entry.

console.log(Math.round(-0.5))   // -0
console.log(Math.round(-0.3))   // -0
console.log(Math.round(-0.1))   // -0
console.log(Math.round(-0.01))  // -0

// Object.is distinguishes +0 / -0
console.log(Object.is(Math.round(-0.3), -0))  // true
console.log(Object.is(Math.round(-0.3), 0))   // false

// 1 / -0 === -Infinity, 1 / 0 === Infinity — another way to probe sign
console.log(1 / Math.round(-0.3))  // -Infinity
console.log(1 / Math.round(0.3))   // Infinity (0.3 < 0.5 → +0)

// Regression guards — round-half-toward-+∞ unchanged
console.log(Math.round(0.5))    // 1
console.log(Math.round(1.5))    // 2
console.log(Math.round(-1.5))   // -1 (spec: closer to +∞)
console.log(Math.round(-2.5))   // -2
console.log(Math.round(-0.6))   // -1 (outside [-0.5, 0))
console.log(Math.round(-0.5001)) // -1 (just outside the carve-out)
console.log(Math.round(0))      // 0
console.log(Math.round(-0))     // -0
console.log(Math.round(2.7))    // 3
console.log(Math.round(-3.7))   // -4
