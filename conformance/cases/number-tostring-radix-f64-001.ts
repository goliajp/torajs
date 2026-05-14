// V3-18 wedge — `n.toString(radix)` for f64 receivers per JS
// spec §21.1.3.6 / §6.1.6.1.13. Pre-fix tora's lower fell
// through to the default f64_to_str(double) which had a 1-arg
// ABI; passing the radix as a 2nd arg crashed LLVM verify
// with 'Incorrect number of arguments passed to called
// function'. Pure compile-time crash — anything that flowed a
// non-integer-valued f64 (i.e. an actual fractional literal,
// or any expression returning f64) into .toString(radix)
// failed at build time.
//
// Implementation:
// * runtime_str.c gains __torajs_num_to_string_radix_f
//   (double, int64) — encodes the integer part via the
//   existing radix_i helper, then a multiply-extract loop
//   over the fractional part (cap 52 digits, the worst-case
//   mantissa precision in radix 2). NaN / Infinity / -Infinity
//   preserve the canonical formatter outputs.
// * ssa_lower's primitive-toString dispatch routes f64
//   receivers with a radix arg through the new helper. The
//   existing i64+radix path stays as the fast lane for
//   integer literals like (255).toString(16).
// * The function-id is also added to the str-drop ownership
//   list so its return value participates in refcount the
//   same way the integer counterpart does.
//
// Subtlety: bun and other major engines emit one extra
// fractional digit and round-half-to-even based on the
// truncated remainder. tora's MVP cap is 52 digits with no
// round step, so very long fractional radix outputs may
// differ in the last 1-2 digits (e.g. (0.1).toString(2)
// gives ...1001 vs bun's ...1101). Cases at <= 16 fractional
// digits agree byte-for-byte, which covers the canonical TS
// usage. The trailing-digit round refinement is a follow-up
// substrate item.

// Fractional-radix output — the crash epicenter.
console.log((10.5).toString(16))               // a.8
console.log((0.5).toString(2))                 // 0.1
console.log((-10.5).toString(16))              // -a.8

// Integer-valued f64 in the f64 path — same digits as the
// i64 path because the helper detects integer-valued and
// delegates through num_to_string_radix_i.
console.log((255.0).toString(16))              // ff
console.log((-255.0).toString(16))             // -ff

// i64 receiver regression — must keep using the existing
// fast path.
console.log((10).toString(16))                 // a
console.log((-10).toString(16))                // -a
console.log((255).toString(16))                // ff
console.log((0).toString(16))                  // 0

// Special values — pass through unchanged.
console.log(NaN.toString(16))                  // NaN
console.log(Infinity.toString(2))              // Infinity
console.log((-Infinity).toString(8))           // -Infinity

// Various radices — sanity check the digits[] table covers
// the full spec range [2, 36].
console.log((123.456).toString(2).slice(0, 16))   // 1111011.0111010
console.log((1234567).toString(36))               // bun: qglj
console.log((0.5).toString(36))                   // 0.i

// Default radix == 10 — falls through to the existing
// f64_to_str (no radix), unchanged.
console.log((10.5).toString())                 // 10.5
console.log((0.5).toString())                  // 0.5
