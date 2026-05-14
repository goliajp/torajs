// V3-18 wedge — Array.fill honors negative start / end per
// JS spec §22.1.3.6: each is normalised through
// ToIntegerOrInfinity, then for index n: if n < 0,
// n = max(len + n, 0); if n >= len, n = len. Pre-fix tora's
// non-Copy path used clamp_i64_to_range(0, len) which mapped
// negatives straight to 0, and the Copy path delegated to
// the C runtime arr_fill which also only does plain min/max
// — same blind spot. Negative start / end is the canonical
// TS pattern for "fill the last N entries" and shows up
// often in init / mask code.
//
// Implementation:
// * ssa_lower's fill lowering normalises start and end via
//   the relative_to_len helper added in the copyWithin wedge
//   (n < 0 ? n + len : n, then clamp [0, len]).
// * Length is loaded once up-front and reused for both
//   normalisations and the default end value.
// * Both Copy (delegates to arr_fill) and non-Copy (per-slot
//   drop-old + store-new + inc-new loop) paths see canonical
//   [0, len] indices, so the runtime helper's own clamp is a
//   no-op for the wedge cases.

// Negative start — fill the tail.
let xs: number[] = [1, 2, 3, 4, 5]
xs.fill(0, -3)
console.log(xs)                              // [ 1, 2, 0, 0, 0 ]

// Negative end — leave the tail intact.
let ys: number[] = [1, 2, 3, 4, 5]
ys.fill(9, 1, -1)
console.log(ys)                              // [ 1, 9, 9, 9, 5 ]

// Negative start AND end.
let zs: number[] = [1, 2, 3, 4, 5]
zs.fill(7, -3, -1)
console.log(zs)                              // [ 1, 2, 7, 7, 5 ]

// Out-of-range negatives clamp to 0.
let x4: number[] = [1, 2, 3, 4, 5]
x4.fill(8, -99)
console.log(x4)                              // [ 8, 8, 8, 8, 8 ]

// Out-of-range positives clamp to len.
let x5: number[] = [1, 2, 3, 4, 5]
x5.fill(8, 0, 99)
console.log(x5)                              // [ 8, 8, 8, 8, 8 ]

// Reversed range — no-op.
let x6: number[] = [1, 2, 3, 4, 5]
x6.fill(8, 3, 1)
console.log(x6)                              // [ 1, 2, 3, 4, 5 ]

// Non-Copy (string array) with negative bounds — refcount
// path also uses the normalised indices.
let strs: string[] = ["a", "b", "c", "d", "e"]
strs.fill("X", -2)
console.log(strs)                            // [ a, b, c, X, X ]

let strs2: string[] = ["a", "b", "c", "d", "e"]
strs2.fill("X", 1, -1)
console.log(strs2)                           // [ a, X, X, X, e ]

// Positive regression (existing path stays correct).
let p1: number[] = [1, 2, 3, 4, 5]
p1.fill(0)
console.log(p1)                              // [ 0, 0, 0, 0, 0 ]

let p2: number[] = [1, 2, 3, 4, 5]
p2.fill(0, 1)
console.log(p2)                              // [ 1, 0, 0, 0, 0 ]

let p3: number[] = [1, 2, 3, 4, 5]
p3.fill(0, 1, 3)
console.log(p3)                              // [ 1, 0, 0, 4, 5 ]
