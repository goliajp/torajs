// V3-18 wedge — global isFinite / isNaN per JS spec §19.2.3 /
// §19.2.4 apply ToNumber to the argument before testing the
// predicate (intentional contrast with the strict
// Number.isFinite / Number.isNaN namespaced methods that don't
// coerce — see check-number-ispredicates-001 fixture).
//
// Pre-fix tora's signature was `(Number) -> Boolean` so calls
// with non-Number args bounced at typecheck with
// 'isFinite arg must be number, got String' / similar — wrong
// per spec and broke the canonical TS pattern of feeding
// loose / mixed-type values through these predicates (very
// common in code that copies idioms from JS).
//
// Implementation:
// * check.rs accepts any arg type (still drives type_of so
//   any internal type error still surfaces). Returns Boolean.
// * ssa_lower applies ToNumber per the same coerce paths
//   Number(x) ctor takes (m1.h.9 / m1.f):
//     Number / Bool / Null   → cleanly mapped to f64 / i64
//     Str / Substr           → __torajs_str_to_number
//                              (strtod, NaN on parse failure)
//     Other heap types       → ToNumber → NaN per spec, so
//                              isFinite returns false,
//                              isNaN returns true; emitted
//                              as ConstBool(name == "isNaN")
// * Refcounted args (Str / Substr / heap fallback) get a
//   drop-on-fresh-owned to avoid leaking after the helper
//   reads them. Mirrors the lifetime handling in Number(s).

// String coercion — the canonical idiom.
console.log(isFinite("3"))                    // true
console.log(isFinite("3.14"))                 // true
console.log(isFinite("  5  "))                // true   ws-trimmed
console.log(isFinite("abc"))                  // false  → NaN
console.log(isFinite(""))                     // true   → 0
console.log(isFinite("Infinity"))             // false
console.log(isFinite("-Infinity"))            // false

// Number / Bool / Null mappings.
console.log(isFinite(3))                      // true
console.log(isFinite(3.14))                   // true
console.log(isFinite(Infinity))               // false
console.log(isFinite(-Infinity))              // false
console.log(isFinite(NaN))                    // false
console.log(isFinite(true))                   // true   → 1
console.log(isFinite(false))                  // true   → 0
console.log(isFinite(null))                   // true   → 0

// isNaN — same coerce path, opposite predicate.
console.log(isNaN(NaN))                       // true
console.log(isNaN("NaN"))                     // true   → NaN
console.log(isNaN("abc"))                     // true   → NaN
console.log(isNaN("3"))                       // false  → 3
console.log(isNaN(""))                        // false  → 0
console.log(isNaN(3))                         // false
console.log(isNaN(true))                      // false  → 1
console.log(isNaN(null))                      // false  → 0
console.log(isNaN(Infinity))                  // false

// Refcounted args — verify no leak when the helper consumes
// the borrow.
console.log(isFinite("freshly-allocated"))    // false
console.log(isNaN("not a number at all"))     // true

// Round-trip: isFinite(s) ≡ !Number.isNaN(Number(s)) &&
// !!Number.isFinite(Number(s)) for any string s, which is
// the spec's exact reduction.
let s = "42"
console.log(isFinite(s))                       // true
console.log(isNaN(s))                          // false
let bad = "xyz"
console.log(isFinite(bad))                     // false
console.log(isNaN(bad))                        // true
