// Array.prototype.lastIndexOf(needle, fromIndex) boundary regression.
// Spec §22.1.3.19 step 4: pos-path `k = min(fromIndex, len - 1)`.
//
// Pre-fix the typed-Arr SSA lowering did `end = clamp(eff + 1, 0, len)`
// AFTER the +1 — so when eff = i64::MAX (fptosi of Infinity saturates
// to i64::MAX), eff + 1 wrapped to i64::MIN, the `> len` clamp missed,
// and the scan range [0, negative) came out empty. Fix: branch on
// `eff >= len` first so overflow can't happen (end = len when eff is
// out of range, end = eff + 1 when in range).
//
// test262 hits (rotation 105 verdict = bug:exit 1):
//   Array/prototype/lastIndexOf/15.4.4.15-5-12
//
// indexOf / includes never went through +1 and stay unchanged; we
// pin their Infinity behaviour anyway so a future refactor can't
// silently regress them.

let xs: number[] = [10, 20, 30, 20, 10]

// lastIndexOf — the fix epicenter.
console.log(xs.lastIndexOf(20, Infinity))    // 3   fromIndex clamped to len-1
console.log(xs.lastIndexOf(10, Infinity))    // 4
console.log(xs.lastIndexOf(30, Infinity))    // 2
console.log(xs.lastIndexOf(99, Infinity))    // -1  no match, still scans whole array
console.log(xs.lastIndexOf(20, -Infinity))   // -1  ToIntegerOrInfinity(-Inf) < 0 ⇒ end = 0
console.log(xs.lastIndexOf(10, -Infinity))   // -1
console.log(xs.lastIndexOf(20, NaN))         // -1  ToIntegerOrInfinity(NaN) = 0 ⇒ end = 1, no 20
console.log(xs.lastIndexOf(10, NaN))         //  0  end = 1, [0] is 10
console.log(xs.lastIndexOf(20, 4294967295))  // 3   fromIndex >> len still clamps
console.log(xs.lastIndexOf(20, 4294967296))  // 3   same, one past uint32 max

// indexOf(v, Infinity) — pre-existing behaviour, pinned.
console.log(xs.indexOf(20, Infinity))        // -1  start past end
console.log(xs.indexOf(20, -Infinity))       //  1  neg clamped to 0
console.log(xs.indexOf(20, NaN))             //  1  ToInteger(NaN) = 0

// includes(v, Infinity) — same normalize path as indexOf.
console.log(xs.includes(20, Infinity))       // false
console.log(xs.includes(20, -Infinity))      // true
console.log(xs.includes(20, NaN))            // true

// String array — same lowering, verify str_eq dispatch.
let ns: string[] = ["a", "b", "a", "c", "a"]
console.log(ns.lastIndexOf("a", Infinity))   // 4
console.log(ns.lastIndexOf("z", Infinity))   // -1
console.log(ns.lastIndexOf("a", -Infinity))  // -1
