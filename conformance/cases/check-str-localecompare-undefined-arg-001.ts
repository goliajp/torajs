// S211 — `s.localeCompare(undefined)` per ES §22.1.3.10 step 4:
// thatStr = ToString(thatValue) = "undefined". Pre-fix declared
// `(String) -> Number` rejected the typed-Undefined arg with
// "argument 0: expected String, got Undefined". ssa_lower inline-
// substitutes the interned "undefined" literal so the helper
// sees a valid Str needle.

// "hello" < "undefined" lexicographically → -1.
console.log("hello".localeCompare(undefined))
// "z" > "undefined" → 1.
console.log("z".localeCompare(undefined))
// "undefined" === "undefined" → 0.
console.log("undefined".localeCompare(undefined))

// Regression checks: explicit string arg unchanged.
console.log("a".localeCompare("a"))
console.log("a".localeCompare("b"))
console.log("b".localeCompare("a"))
