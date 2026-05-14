// V3-18 wedge — String.charAt / charCodeAt / codePointAt
// 0-arg form per JS spec §22.1.3.4 / §22.1.3.5 / §22.1.3.6:
// the `pos` argument defaults to 0 when omitted. Pre-fix tora
// declared the signatures with one required param so 0-arg
// calls bounced at the unified arity check with 'expected 1
// argument(s), got 0'. The 0-arg idiom is canonical TS for
// 'first char of a string' and shows up in micro-utilities all
// over the place.
//
// Implementation:
// * check.rs special-cases the three methods before the
//   generic arity check: typecheck-only pass through for the
//   missing-arg shape on Str receivers; returns String for
//   charAt and Number for charCodeAt / codePointAt.
// * ssa_lower hoists a 0-arg branch above the existing 1-arg
//   dispatch:
//   - Str.charAt()         → str_char_at(s, 0)
//   - Str.charCodeAt()     → str_char_code_at(s, 0)
//   - Str.codePointAt()    → str_char_code_at(s, 0)
//   - Substr.charAt()      → substr_slice(v, 0, 1)  (hoisted
//                             above the generic Substr
//                             dispatch — it has no view-aware
//                             charAt path)
//   - Substr.charCodeAt() / codePointAt() — argv pad of
//                             ConstI64(0) inside the view-
//                             aware substr_char_code_at branch
//                             so the same 2-arg ABI holds.

// Str receiver — the canonical case.
console.log("hello".charAt())                  // h
console.log("hello".charAt(0))                 // h    1-arg regression
console.log("hello".charAt(1))                 // e
console.log("hello".charAt(99))                // ""   OOB

console.log("hello".charCodeAt())              // 104
console.log("hello".charCodeAt(0))             // 104  1-arg regression
console.log("hello".charCodeAt(4))             // 111

console.log("ABC".codePointAt())               // 65
console.log("ABC".codePointAt(0))              // 65   1-arg regression
console.log("ABC".codePointAt(2))              // 67

// Substr receiver — split returns Substr handles into the
// source's bytes; the 0-arg form must work without
// materializing.
let parts = "abc,def".split(",")
console.log(parts[0].charAt())                 // a
console.log(parts[1].charAt())                 // d
console.log(parts[0].charCodeAt())             // 97
console.log(parts[1].codePointAt())            // 100

// Empty string — charAt OOB returns "".
console.log("".charAt())                       // ""

// "".charCodeAt() / codePointAt() should give NaN per spec
// when the string is empty (no codeunit at index 0); tora's
// byte-Str path returns 0 instead — separate substrate item
// (would need NaN tagging on the integer-typed return). Not
// in this wedge's scope; the wedge intent is just 'the 0-arg
// shape parses and dispatches'.

