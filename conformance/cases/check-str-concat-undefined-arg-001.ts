// S212 — `s.concat(undefined)` per ES §22.1.3.4 step 3.a:
// each arg is ToString'd, undefined → "undefined". Pre-fix tora
// rejected typed-Undefined operands at the declared `(String)
// -> String` arity-1 dispatch ("argument 0: expected String,
// got Undefined") and at the multi-arg carve-out's strict-
// string check ("String.concat args must be string, got
// Undefined").

// 1-arg undefined.
console.log("hello".concat(undefined))
// 1-arg empty receiver.
console.log("".concat(undefined))
// 2-arg mixed with explicit undefined in the middle.
console.log("hello".concat("a", undefined))
console.log("hello".concat(undefined, "b"))
// 3-arg with multiple undefineds interleaved.
console.log("x".concat("a", undefined, "b"))
console.log("x".concat(undefined, undefined, undefined))

// Regression checks: all-string path unchanged.
console.log("a".concat("b"))
console.log("a".concat("b", "c"))
