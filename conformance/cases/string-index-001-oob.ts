// V3-18 m1.h.44 — `s[i]` (string subscript) bounds-checked.
// Mirror of m1.h.37 (charAt): pre-fix tora's Index path on
// Type::Str called substr_create directly with the user's idx,
// so `"abc"[5]` stored a garbage offset and printed bytes from
// past the parent's data.
//
// Now: route Str + Index through the same __torajs_str_char_at
// helper as charAt — returns a length-0 Substr view for OOB.
// Spec divergence: bun returns `undefined` for OOB which throws
// when used with `.length` etc; tora returns the empty Substr,
// which keeps the read total via length-0 (no Type::Undefined
// substrate yet). This fixture only checks the in-range cases
// since OOB semantics differ at the substrate level.

let s = "abc"
console.log(s[0])         // a
console.log(s[1])         // b
console.log(s[2])         // c

// In-range still zero-copy via Substr view.
console.log(s[0] + s[1] + s[2])   // abc

// charAt and [] consistent for in-range.
console.log(s[0] === s.charAt(0))   // true
