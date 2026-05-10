// V3-18 m1.h.35 — Array.prototype.slice with optional args and
// negative indices. Per JS spec §22.1.3.25:
//   xs.slice()           = xs.slice(0, len)
//   xs.slice(start)      = xs.slice(start, len)
//   xs.slice(-N)         = xs.slice(len - N, len)  (N <= len)
//   xs.slice(-N, -M)     = xs.slice(len - N, len - M)
//
// Pre-fix tora declared slice with 2 fixed params (so 0/1-arg
// calls hit the arity check) and the runtime helper clamped
// negative indices to 0 instead of normalizing via `len + i`.
//
// Fix: check.rs accepts 0/1/2 args; ssa_lower fills in defaults
// (start = 0, end = len). Runtime helper applies the spec's
// negative-index normalization.

let a = [1, 2, 3, 4, 5]

console.log(a.slice())                      // [ 1, 2, 3, 4, 5 ]
console.log(a.slice(0, 100))                // [ 1, 2, 3, 4, 5 ]
console.log(a.slice(100))                   // []

console.log(a.slice(2))                     // [ 3, 4, 5 ]
console.log(a.slice(-2))                    // [ 4, 5 ]
console.log(a.slice(-100, 100))             // [ 1, 2, 3, 4, 5 ]
console.log(a.slice(-3, -1))                // [ 3, 4 ]
console.log(a.slice(2, -1))                 // [ 3, 4 ]
console.log(a.slice(0, -3))                 // [ 1, 2 ]

// String[] receivers behave the same.
let s = ["a", "b", "c", "d"]
console.log(s.slice(-2))                    // [ "c", "d" ]
