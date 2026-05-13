// V3-18 wedge — String.prototype.toString / toLocaleString
// per JS spec §22.1.3.27 / §22.1.3.31. Both return the
// receiver string itself (identity); the spec calls them out
// because they're on String.prototype to satisfy the generic
// `Object.prototype.toString` shadow chain. tora already had
// the matching wires for Number / Boolean / BigInt / Symbol /
// Array but had skipped String, so the canonical TS pattern
// `s.toString()` (often used to cover unknown-string-or-other
// branches) hit 'no member .toString on type String'.
//
// Implementation:
// * check.rs adds Type::String → toString / toLocaleString to
//   the same pattern arm that already handles String.valueOf,
//   typed as `() => string` (matches Number/Boolean shape).
// * ssa_lower's primitive-toString dispatch detects Str /
//   Substr receivers and just returns the operand unchanged —
//   no new runtime helper, no copy.

// Direct on literal.
console.log("hello".toString())                // hello
console.log("".toString())                     // (empty line)

// On a binding — Type::Str path.
let s = "abc"
console.log(s.toString())                      // abc
console.log(s.toString().length)               // 3

// Chain through toString back into other String methods.
console.log("xyz".toString().toUpperCase())    // XYZ
console.log("Hello World".toString().split(" ").join("_"))  // Hello_World

// In a generic-typed parameter — pattern that motivated this
// (TS code that calls .toString() on an inferred unknown).
function p(x: string): void { console.log(x.toString()) }
p("test")
p("")

// toLocaleString — same path.
console.log("hi".toLocaleString())             // hi

// On a Substr receiver (split returns Substrs that lower to
// the same SSA Str path) — verifies the Substr arm.
let parts = "alpha,beta,gamma".split(",")
for (let part of parts) console.log(part.toString())
// alpha\nbeta\ngamma

// Round-trip through valueOf+toString chains (both already
// shipped, sanity-check they compose).
console.log("round".valueOf().toString())      // round
console.log("trip".toString().valueOf())       // trip
