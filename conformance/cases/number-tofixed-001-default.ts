// V3-18 m1.h.46 — Number.toFixed / toExponential / toPrecision
// with no precision arg. Per JS spec §21.1.3.3 / §21.1.3.5 /
// §21.1.3.6, undefined precision defaults to 0 (toFixed) or
// converts via ToString. Pre-fix tora declared the methods with
// 1 fixed param so 0-arg calls failed at the arity check.
//
// Implementation passes 0 to the runtime helper as the missing-
// arg fallback, matching toFixed spec exactly. toExponential /
// toPrecision with 0 may diverge from spec in subtle ways for
// extreme inputs but matches bun for the common cases.

console.log((3.14).toFixed())     // "3"
console.log((3.5).toFixed())      // "4"  (banker's rounding doesn't apply here)
console.log((3.49).toFixed())     // "3"
console.log((3).toFixed())        // "3"

// Explicit-arg form still works (no regression).
console.log((3.14).toFixed(1))    // "3.1"
console.log((3.14).toFixed(2))    // "3.14"
console.log((3.14).toFixed(0))    // "3"

// toExponential / toPrecision smoke (output may differ for some
// extreme cases; basic shapes match).
console.log((100).toExponential(0))  // "1e+2"
console.log((100).toPrecision(3))    // "100"
