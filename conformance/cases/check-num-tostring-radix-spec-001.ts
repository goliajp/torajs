// `Number.prototype.toString(radix)` shortest-round-trip per ES §21.1.3.6.
// Pre-fix `to_string_radix_f` had two gaps vs bun/V8 spec quality:
//   (a) integer part > i64::MAX flowed through `int_part as i64` saturating
//       cast, so (1e20).toString(2) emitted 63 ones instead of the correct
//       67-bit representation.
//   (b) fractional part hard-capped at 52 digits with no round step, so
//       (0.1).toString(2) emitted 52 digits ending '...1001' vs bun's 55
//       digits ending '...1101' (V8 emits until the next digit would fall
//       below ulp/2 — shortest round-trip + banker's tie-break).
// Fix: V8-style algorithm in tostring.rs — integer part walks via f64
// fmod/div loop (handles values past i64::MAX), fractional part scales
// `frac` and `delta = ulp(d)/2` together, emitting until `frac <= delta`
// with banker's rounding + carry propagation through the digit buffer.

// (a) Large-integer-valued f64 — exceeds i64::MAX.
console.log((1e20).toString(2))           // 67 binary digits
console.log((1e15).toString(16))          // ~13 hex digits
console.log((Number.MAX_SAFE_INTEGER).toString(2))   // 53 ones

// (b) Fractional values needing shortest round-trip beyond 52 digits.
console.log((0.1).toString(2))            // 0.0001100110011...1101 (55 digits)
console.log((0.3).toString(2))            // 0.010011001100...110011 (54 digits)

// Round-trip + carry: 0.5 in base-2 should still be 0.1 (no carry trigger).
console.log((0.5).toString(2))            // 0.1
console.log((0.25).toString(2))           // 0.01

// Mixed integer + fractional — verifies assembly with delimiter.
console.log((3.14).toString(2))           // 11.001000111101...
console.log((255.5).toString(16))         // ff.8

// Negative + fractional.
console.log((-0.1).toString(2))           // -0.0001100110011...1101

// Various radices, all in range.
console.log((10.5).toString(36))          // a.i
console.log((123.456).toString(16))       // 7b.74bc6a7ef9dc
