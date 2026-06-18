// S159 — `Math.f16round(x)` (ES 2025 §21.3.2.31). Rounds `x` to
// the nearest IEEE-754 binary16 (Float16) representable value and
// returns the result as a binary64 (Number). Companion to
// `Math.fround` for the half-precision tier.
//
// Rust stable doesn't expose `f16` yet (the unstable
// `#[feature(f16)]` is gated), and torajs-num runs a 0-external-
// dep policy, so the runtime helper open-codes the binary32 →
// binary16 conversion with round-to-nearest-ties-to-even per
// IEEE-754, then expands back through binary32 → binary64.

// Basic finite values.
console.log(Math.f16round(1.337));
console.log(Math.f16round(0));
console.log(Math.f16round(-0));
console.log(Math.f16round(-1));
console.log(Math.f16round(0.5));
console.log(Math.f16round(1.5));

// Special non-finite values are preserved.
console.log(Math.f16round(NaN));
console.log(Math.f16round(Infinity));
console.log(Math.f16round(-Infinity));

// Boundary: max finite binary16 (65504), one above (rounds down),
// and the overflow threshold (rounds to Infinity).
console.log(Math.f16round(65504));
console.log(Math.f16round(65505));
console.log(Math.f16round(65520));

// 0.1 — classic precision loss demo.
console.log(Math.f16round(0.1));

// Subnormal / underflow region.
console.log(Math.f16round(6.10352e-5));
console.log(Math.f16round(6.0e-5));
console.log(Math.f16round(1e-8));

// Overflow.
console.log(Math.f16round(1e10));
