// V3-18 wedge — Number.isFinite / isNaN / isInteger /
// isSafeInteger per JS spec §21.1.2.2 / §21.1.2.4 /
// §21.1.2.3 / §21.1.2.5 do NOT coerce their argument
// (intentional contrast with the global isFinite / isNaN
// which DO coerce). They return true only when the arg is
// a Number that satisfies the predicate; for non-Number
// arguments (string / boolean / null / object / array /
// etc.) they return false statically.
//
// Pre-fix tora declared the signature as `(Number) -> Boolean`
// so calls with non-Number args bounced at the typecheck with
// 'argument 0: expected Number, got String' — wrong per spec
// and broke the canonical TS feature-detection idiom
// `if (Number.isFinite(maybe)) { /* maybe is a finite number */ }`.
//
// Implementation:
// * check.rs special-cases the four Number.is* methods before
//   the generic arity check: forces type_of on the arg (so
//   internal type errors still surface) but returns Boolean
//   regardless of arg type.
// * ssa_lower's Number.is* dispatch detects non-numeric SSA
//   types (anything other than I64 / F64 / I32) and returns
//   ConstBool(false) directly. Refcounted args (string /
//   array / struct) get a drop-on-fresh-owned to avoid
//   leaking when the spec-static-false path replaces the
//   normal call sequence.

// isFinite — false for any non-Number, regardless of value.
console.log(Number.isFinite(42))               // true
console.log(Number.isFinite(3.14))             // true
console.log(Number.isFinite(Infinity))         // false
console.log(Number.isFinite(-Infinity))        // false
console.log(Number.isFinite(NaN))              // false
console.log(Number.isFinite("3"))              // false
console.log(Number.isFinite(true))             // false
console.log(Number.isFinite(null))             // false
console.log(Number.isFinite([1]))              // false

// isNaN — true ONLY for the literal NaN value, false for
// non-Number (contrast with the global isNaN which would
// coerce "NaN" through ToNumber).
console.log(Number.isNaN(NaN))                 // true
console.log(Number.isNaN(42))                  // false
console.log(Number.isNaN(Infinity))            // false
console.log(Number.isNaN("NaN"))               // false  not coerced
console.log(Number.isNaN(true))                // false
console.log(Number.isNaN(null))                // false

// isInteger — true for finite integer Numbers only.
console.log(Number.isInteger(42))              // true
console.log(Number.isInteger(3.14))            // false
console.log(Number.isInteger(Infinity))        // false
console.log(Number.isInteger(NaN))             // false
console.log(Number.isInteger("3"))             // false
console.log(Number.isInteger(true))            // false

// isSafeInteger — integer + |x| ≤ 2^53 - 1.
console.log(Number.isSafeInteger(42))          // true
console.log(Number.isSafeInteger(2 ** 53))     // false  exceeds safe range
console.log(Number.isSafeInteger(3.14))        // false
console.log(Number.isSafeInteger(NaN))         // false
console.log(Number.isSafeInteger("3"))         // false
console.log(Number.isSafeInteger(null))        // false

// Refcounted args — verify the lowering doesn't leak when
// spec-false replaces the normal call. Tested by passing a
// fresh string / fresh array literal (both heap-owned).
console.log(Number.isFinite("freshly-allocated"))  // false
console.log(Number.isFinite([10, 20, 30]))         // false

// Used in feature-detection idiom — call directly on a Number
// receiver (the source intent for the wedge). The number|string
// union shape that motivated this isn't yet a parseable type
// in tora, so we exercise both arg-types via separate calls.
console.log(Number.isFinite(42))               // true     finite-number
console.log(Number.isFinite("hello"))          // false    other
console.log(Number.isFinite(NaN))              // false    other
