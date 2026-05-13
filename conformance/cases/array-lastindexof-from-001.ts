// V3-18 wedge — `Array.lastIndexOf(needle, fromIndex)` honors
// the `fromIndex` second argument per JS spec §22.1.3.16.
// Pre-fix tora typechecked the 2-arg form (m1.h.49) but the SSA
// lower silently ignored `fromIndex`: lastIndexOf reused the
// indexOf forward-scan loop with `keep last match`, so when
// `fromIndex < len-1` the loop kept scanning to the end of the
// array and returned the rightmost match — wrong, since spec
// says lastIndexOf walks *backwards* from `fromIndex` to 0 and
// returns the first match it finds (i.e. the rightmost match
// that is ≤ fromIndex).
//
// Implementation:
// * ssa_lower folds reverse-scan onto the existing forward
//   scan: instead of one start-only `i_slot`, allocate both
//   `i_slot` and `end_slot`. For lastIndexOf, args[1] feeds
//   `end_slot = clamp(effective_from + 1, 0, len)` (start
//   stays at 0); for indexOf/includes, args[1] feeds `i_slot`
//   (end stays at len). The body still records the last match,
//   so for lastIndexOf the [0, from+1) range with keep-last
//   matches the spec's reverse walk exactly.
// * Negative-from normalization differs by direction: for
//   indexOf/includes a negative effective < 0 clamps to 0
//   (start from the front); for lastIndexOf it clamps to -1
//   (so end_bound = 0, no iterations, returns -1 — matches
//   spec when `len + from < 0`).
//
// All three methods (indexOf / lastIndexOf / includes) now
// agree with bun across positive, negative, and out-of-range
// fromIndex values.

let xs: number[] = [10, 20, 30, 20, 10]

// lastIndexOf — the bug epicenter.
console.log(xs.lastIndexOf(20))                // 3
console.log(xs.lastIndexOf(20, 0))             // -1   only [0,1) scanned
console.log(xs.lastIndexOf(10, 0))             // 0    [0,1) scanned, hit
console.log(xs.lastIndexOf(20, 2))             // 1    [0,3) keep last
console.log(xs.lastIndexOf(10, -1))            // 4    eff=4
console.log(xs.lastIndexOf(20, -2))            // 3    eff=3
console.log(xs.lastIndexOf(20, -3))            // 1    eff=2; [0,3) keep last
console.log(xs.lastIndexOf(20, 99))            // 3    end clamped to len
console.log(xs.lastIndexOf(20, -99))           // -1   eff<0 → end=0

// indexOf — pre-existing fromIndex path stays correct.
console.log(xs.indexOf(20, 2))                 // 3
console.log(xs.indexOf(20, -2))                // 3    start = 3
console.log(xs.indexOf(10, -99))               // 0    neg clamped to 0
console.log(xs.indexOf(10, 99))                // -1   start past end

// includes — same path as indexOf.
console.log(xs.includes(20, 4))                // false
console.log(xs.includes(10, 4))                // true
console.log(xs.includes(10, -1))               // true

// String arrays — exercises the str_eq dispatch.
let names: string[] = ["a", "b", "a", "c", "a"]
console.log(names.lastIndexOf("a"))            // 4
console.log(names.lastIndexOf("a", 2))         // 2
console.log(names.lastIndexOf("a", 1))         // 0
console.log(names.lastIndexOf("z", 99))        // -1

// 0-arg lastIndexOf still returns the rightmost match.
let ys: number[] = [1, 2, 1, 2, 1]
console.log(ys.lastIndexOf(1))                 // 4
console.log(ys.lastIndexOf(2))                 // 3
