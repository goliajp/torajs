// V3-18 wedge — String.prototype.split optional 2nd arg per
// JS spec §22.1.3.21:
//   "a,b,c,d".split(",", 2)   → ["a", "b"]
//   "a,b,c".split(",", 0)     → []
//   "hello".split("", 3)      → ["h", "e", "l"]
// Returns the first `limit` substrings (or fewer if the source
// splits into fewer). Pre-fix tora rejected with 'expected 1
// argument(s), got 2' since split was declared with a fixed
// 1-arg signature.
//
// Implementation:
// * check.rs special-cases 2-arg `<str>.split(sep, limit)`
//   when receiver is String, returns Array<String>.
// * ssa_lower's split arm: when args.len() == 2, calls
//   __torajs_str_split, then arr_slice([0, min(limit, len)))
//   to truncate. The arr_slice result still aliases the
//   source's bytes (each element is a Substr view), so no
//   per-substring copy.

console.log("a,b,c,d".split(",", 2))    // [ "a", "b" ]
console.log("a,b,c,d".split(",", 0))    // []
console.log("a,b,c,d".split(",", 99))   // [ "a", "b", "c", "d" ] (limit > len)
console.log("hello".split("", 3))       // [ "h", "e", "l" ]
console.log("solo".split(",", 5))       // [ "solo" ] (no separator found)

// limit-N = exactly N elements when source has at least N.
let parts = "k1=v1;k2=v2;k3=v3".split(";", 2)
console.log(parts.length)               // 2
console.log(parts[0])                   // k1=v1
console.log(parts[1])                   // k2=v2

// Keeps the existing 1-arg form working.
console.log("a,b,c".split(","))         // [ "a", "b", "c" ]
