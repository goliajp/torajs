// V3-18 m1.h.27 — BigInt.prototype.toString() returns the
// decimal representation without the trailing `n` suffix. Per
// ECMA-262 §21.2.3.5 / §21.2.3.6.
//
// Pre-fix tora rejected with "no member \`.toString\` on type
// BigInt" — the runtime helper bigint_to_string already existed
// (used by string-concat coercion) but was not wired through
// the method-call surface.

let x = 5n
console.log(x.toString())            // "5"
console.log((10n).toString())        // "10"
console.log((-7n).toString())        // "-7"
console.log((100n + 200n).toString()) // "300"
console.log(0n.toString())           // "0"

// Used in concat — exercises the canonical no-suffix path.
console.log("count: " + (42n).toString())
console.log((123456789012345678901234567890n).toString())
