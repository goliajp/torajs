// V3-01 (T-25 follow-up) — `**` exponent, both Number and BigInt.
// JS spec: right-associative, precedence above mul/div/mod.
// Number ** Number → Number (libm pow).
// BigInt ** BigInt → BigInt (square-and-multiply).
// Negative-exponent BigInt → RangeError (not exercised here as the
// fixture compares stdout against bun; a wrapper that catches the
// error lands when V3-07 ships `as` cast + the test262 push covers
// the exact RangeError message).
//
// Spec also requires unary `-` left of `**` to be parenthesized
// (`-2 ** 2` is a SyntaxError in spec-strict bun); we accept the
// `-(2 ** 2)` reading in v0.7 and tighten in V3-18 alongside test262.
// Fixture uses paren-form everywhere so byte-for-byte vs bun.

// Number `**` works end-to-end (libm pow); we don't byte-compare
// it here because tora's f64 → string formatter prints whole-
// number f64s in scientific form (`1.024e+03`) while bun prints
// them as integers — that's a separate stringification gap, not
// a `**` correctness gap. The BigInt branch below is what V3-01
// actually adds.
console.log(0.5 ** 3)            // 0.125 — formatter agrees on fractional values

console.log(2n ** 10n)
console.log(2n ** 0n)
console.log(2n ** 100n)
console.log((-2n) ** 3n)         // odd exp: keeps sign → -8n
console.log((-2n) ** 4n)         // even exp: positive  → 16n
console.log(0n ** 0n)            // spec quirk: 1n
console.log(123n ** 5n)
console.log(2n ** 3n ** 2n)      // right-assoc → 2n ** 9n = 512n
