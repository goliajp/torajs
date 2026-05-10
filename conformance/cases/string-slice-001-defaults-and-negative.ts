// V3-18 m1.h.36 — String.slice / substring with optional args
// and negative indices. Per JS spec §21.1.3.21:
//   s.slice()       = s.slice(0, s.length)
//   s.slice(start)  = s.slice(start, s.length)
//   s.slice(-N)     = s.slice(s.length - N, s.length)  (clamped to 0)
//
// Pre-fix tora declared slice/substring with 2 fixed params and
// the str_slice IR clamped negative indices to 0 instead of
// normalizing via len + i. So `s.slice()` failed at the call
// arity check and `s.slice(-2)` returned the whole string.

let s = "hello"
console.log(s.slice())                  // hello
console.log(s.slice(1))                 // ello
console.log(s.slice(1, 3))              // el
console.log(s.slice(-2))                // lo
console.log(s.slice(-3, -1))            // ll
console.log(s.slice(2, -1))             // ll

console.log(s.substring())              // hello
console.log(s.substring(1))             // ello
console.log(s.substring(2, 4))          // ll

// Substr receivers behave the same.
let parts = "a-bcdef".split("-")
console.log(parts[1].slice())           // bcdef
console.log(parts[1].slice(1))          // cdef
console.log(parts[1].slice(-2))         // ef
