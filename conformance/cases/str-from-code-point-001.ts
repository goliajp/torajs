// P11.1-S6 — `String.fromCodePoint` spec parity (ES §22.1.2.2).
//
// Pre-S6: `fromCodePoint` shared the runtime intrinsic of
// `fromCharCode`, which masks the input to 16 bits (`n & 0xFFFF`).
// `fromCodePoint(0x1F600)` therefore lost the supplementary plane
// bits and produced a single low surrogate (`0xF600`) instead of
// the spec-mandated surrogate pair (2 UTF-16 LE code units).
// S6 splits the dispatch: `fromCodePoint` now routes to
// `__torajs_str_from_code_point`, which encodes the full codepoint
// (`[0, 0x10FFFF]`) and raises a catchable `RangeError` on invalid
// input.
//
// Coverage:
// - BMP Latin-1 (`A` U+0041)
// - BMP UTF-16 (`中` U+4E2D)
// - Supplementary plane surrogate pair (`😀` U+1F600 → 2 code units)
// - Max supplementary boundary (U+10FFFF)
// - Multi-arg variadic
// - `fromCharCode` regression: still truncates to 1 code unit
// - RangeError path via try/catch (negative + out-of-range)

console.log(String.fromCodePoint(65))
console.log(String.fromCodePoint(0x4e2d))
console.log(String.fromCodePoint(0x1f600))
console.log(String.fromCodePoint(0x1f600).length)
console.log(String.fromCodePoint(0x10ffff).length)
console.log(String.fromCodePoint(65, 66, 67))
console.log(String.fromCodePoint(0x1f600, 0x4e2d, 65))

// fromCharCode regression — 1 code unit per arg, no surrogate
// pair encoding of supplementary inputs.
console.log(String.fromCharCode(65))
console.log(String.fromCharCode(0x1f600).length)
console.log(String.fromCharCode(0x1f600, 0x1f600).length)

try {
  String.fromCodePoint(-1)
  console.log('NO_THROW_NEG')
} catch (_e) {
  console.log('CAUGHT_NEG')
}

try {
  String.fromCodePoint(0x110000)
  console.log('NO_THROW_OVER')
} catch (_e) {
  console.log('CAUGHT_OVER')
}
