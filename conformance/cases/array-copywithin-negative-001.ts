// V3-18 wedge — Array.copyWithin honors negative indices per
// JS spec §22.1.3.3: each of target / start / end is
// normalised through ToIntegerOrInfinity then the relative
// index resolves as
//   if n < 0:  n = max(len + n, 0)
//   if n >= len: n = len
//   else:      n
// Pre-fix tora used a plain min/max clamp_i64_to_range(0, len)
// which mapped any negative input straight to 0, dropping the
// canonical TS pattern of using negatives to count from the
// end (e.g. `xs.copyWithin(-2)` shifts the tail to the front).
//
// Implementation:
// * ssa_lower gains a relative_to_len helper that emits the
//   `n < 0 ? n + len : n` select via condbr+slot+load (no
//   SSA-level Select instruction in tora's IR), then chains
//   through the existing clamp_i64_to_range for the [0, len]
//   clamp.
// * The copyWithin lower path normalises target / start / end
//   through this helper instead of the plain clamp. Length
//   is loaded once up-front and reused for the three normal-
//   isations (no per-arg reload).
// * The non-Copy refcount-aware path inside copyWithin reuses
//   the same lo / hi / to operands the new normalisation
//   produces, so refcount inc / drop on overlapping ranges
//   stays balanced — the wedge is purely an index-rewrite,
//   not a copy-semantics change.

// Negative target — shifts tail to front.
let x1: number[] = [1, 2, 3, 4, 5]
x1.copyWithin(-2)
console.log(x1)                                // [ 1, 2, 3, 1, 2 ]

// Negative start — copies tail to front.
let x2: number[] = [1, 2, 3, 4, 5]
x2.copyWithin(0, -2)
console.log(x2)                                // [ 4, 5, 3, 4, 5 ]

// Negative start AND end — both ends count from the right.
let x3: number[] = [1, 2, 3, 4, 5]
x3.copyWithin(1, -3, -1)
console.log(x3)                                // [ 1, 3, 4, 4, 5 ]

// Negatives that overshoot the length clamp to 0 — no-op.
let x4: number[] = [1, 2, 3, 4, 5]
x4.copyWithin(-99)
console.log(x4)                                // [ 1, 2, 3, 4, 5 ]

let x5: number[] = [1, 2, 3, 4, 5]
x5.copyWithin(0, -99)
console.log(x5)                                // [ 1, 2, 3, 4, 5 ]

// Positives that overshoot clamp to len — no-op.
let x6: number[] = [1, 2, 3, 4, 5]
x6.copyWithin(0, 99)
console.log(x6)                                // [ 1, 2, 3, 4, 5 ]

let x7: number[] = [1, 2, 3, 4, 5]
x7.copyWithin(99)
console.log(x7)                                // [ 1, 2, 3, 4, 5 ]

// Positive target — regression check (existing path).
let p1: number[] = [1, 2, 3, 4, 5]
p1.copyWithin(0)
console.log(p1)                                // [ 1, 2, 3, 4, 5 ] no-op

let p2: number[] = [1, 2, 3, 4, 5]
p2.copyWithin(2)
console.log(p2)                                // [ 1, 2, 1, 2, 3 ]

let p3: number[] = [1, 2, 3, 4, 5]
p3.copyWithin(0, 3)
console.log(p3)                                // [ 4, 5, 3, 4, 5 ]

let p4: number[] = [1, 2, 3, 4, 5]
p4.copyWithin(0, 3, 4)
console.log(p4)                                // [ 4, 2, 3, 4, 5 ]

// Refcount-aware path — string array with negative target
// goes through the inc-src + drop-dst sequence; no leaks.
let strs: string[] = ["a", "b", "c", "d", "e"]
strs.copyWithin(-2)
console.log(strs)                              // [ a, b, c, a, b ]
