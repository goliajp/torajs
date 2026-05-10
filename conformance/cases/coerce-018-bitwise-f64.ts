// V3-18 m1.h.40 — bitwise ops on f64 operands (per JS spec
// §7.1.6 ToInt32). The standard `x | 0` idiom truncates a
// fractional Number to an integer in JS, used pervasively in
// test262 and real code.
//
// Pre-fix tora's lower_binop hard-rejected with
// "bitwise/mod op `BitOr` requires i64 operands" when either
// side was f64. Now: f64 operands route through a ToInt32-like
// FpToSi (constants fold; NaN/Inf → 0).

console.log(3.7 | 0)         // 3
console.log(3.7 & 0xff)      // 3
console.log(-3.7 | 0)        // -3
console.log(NaN | 0)         // 0
console.log(Infinity | 0)    // 0
console.log(-Infinity | 0)   // 0

// Mixed bit ops via the truncate idiom.
console.log(2.9 ^ 0)         // 2
console.log(7.5 << 1)        // 14
console.log(7.5 >> 1)        // 3
// `-1 >>> 0` would yield 4294967295 in JS (uint32) but tora's
// i64 model can't represent that exactly without a separate
// ToUint32 path; gated until V3-21 unsigned-bit substrate.

// Pure-integer paths still work (no regression).
console.log(0xff & 0x0f)      // 15
console.log(0xff ^ 0xaa)      // 85
console.log(1 << 4)           // 16
