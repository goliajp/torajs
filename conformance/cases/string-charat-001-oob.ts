// V3-18 m1.h.37 — String.charAt with out-of-range index returns
// empty string per JS spec §21.1.3.1. Pre-fix tora's Str charAt
// path called substr_create directly with the user's idx, so:
//   "hello".charAt(-1) → garbage bytes (offset = uint64_t(-1))
//   "hello".charAt(100) → garbage bytes past the parent's data
// Substr charAt already routed through substr_slice which clamps,
// so this is the Str-receiver fix.
//
// Add __torajs_str_char_at runtime helper that returns a length-0
// Substr view for OOB; in-range stays a length-1 view (zero copy).

console.log("hello".charAt(-1))      // ""
console.log("hello".charAt(100))     // ""
console.log("hello".charAt(0))       // h
console.log("hello".charAt(4))       // o
console.log("hello".charAt(2))       // l

// .at(-1) follows different spec — already supported, kept for
// regression check.
console.log("hello".at(-1))          // o
console.log("hello".at(0))           // h

// Empty receiver.
console.log("".charAt(0))            // ""
console.log("".charAt(-1))           // ""
console.log("done")
