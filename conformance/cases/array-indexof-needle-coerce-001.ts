// V3-18 wedge — Array.indexOf / lastIndexOf / includes accept
// a needle whose SSA type doesn't match the array's element
// type, per JS spec §7.2.13 StrictEqualityComparison: when
// both elem and needle are typed Number (in JS source space)
// the comparison is well-defined regardless of how tora's
// SSA layer happens to lower them to i64 vs f64. Pre-fix
// tora passed the raw needle straight into the ICmp / FCmp
// dispatch, so an i64 array with an f64 needle (e.g. NaN,
// -0.0, fractional literal, runtime f64 expr) hit an LLVM
// verify error: 'Found FloatValue but expected IntValue'.
//
// Implementation:
// * ssa_lower's indexOf / lastIndexOf / includes loop now
//   bridges the elem ↔ needle types before the compare:
//   - same SSA type → straight-through (the common case).
//   - i64 elem, f64 needle:
//       const f64 with integer value in i64 range → coerce.
//       const f64 NaN / Infinity / fractional → no i64 element
//         can match it, short-circuit to -1 / false.
//       runtime f64 → coerce_to_i64 (matches tora's mixed-
//         BinOp bridging).
//   - f64 elem, i64 needle → promote i64 → f64.
//   - other type pairs (Str / Obj etc.) — unchanged.
//
// Subtlety left for a follow-up: SameValueZero — `includes`
// per spec returns true for `[NaN].includes(NaN)`. tora's
// FCmp(Oeq, NaN, NaN) returns false, so the NaN-in-Array<f64>
// case still gives the wrong answer. Separate substrate item
// (would need an FCmp::Une self-test for the includes-only
// path); this wedge's scope is just stopping the crash.

// Pre-fix epicenter — i64 array with NaN needle.
let xs: number[] = [1, 2, 3]
console.log(xs.indexOf(NaN))                 // -1
console.log(xs.lastIndexOf(NaN))             // -1
console.log(xs.includes(NaN))                // false

// -0 needle on i64 array — coerces to 0 cleanly. Bun and
// tora agree because xs has no 0.
console.log(xs.indexOf(-0))                  // -1
console.log(xs.includes(-0))                 // false

// -0 in an array that DOES contain 0 — SameValueZero says
// -0 === 0. The const-f64 branch coerces -0.0 to ConstI64(0).
let ys: number[] = [0, 1, 2]
console.log(ys.indexOf(-0))                  // 0
console.log(ys.includes(-0))                 // true

// Fractional f64 needle on i64 array — short-circuit, no
// i64 element can equal a fractional value.
console.log(xs.indexOf(2.5))                 // -1
console.log(xs.includes(2.5))                // false

// Integer-valued f64 needle (parser keeps `2.0` as i64
// already, but include the case for clarity).
console.log(xs.indexOf(2.0))                 // 1   matches xs[1]=2
console.log(xs.includes(2.0))                // true

// Infinity needle on i64 array.
console.log(xs.indexOf(Infinity))            // -1
console.log(xs.includes(Infinity))           // false
console.log(xs.indexOf(-Infinity))           // -1

// Regression — same-type i64 path stays correct.
console.log(xs.indexOf(2))                   // 1
console.log(xs.lastIndexOf(2))               // 1
console.log(xs.includes(2))                  // true
console.log(xs.indexOf(99))                  // -1

// Array<f64> literals are a separate substrate item (the
// parser currently routes number[] with f64-valued literals
// through the i64 slot layout, miscompiling the raw bits);
// once that lands we'll re-add fractional-array tests
// alongside the matching SameValueZero NaN-in-Array<f64>
// case noted in the wedge header.
