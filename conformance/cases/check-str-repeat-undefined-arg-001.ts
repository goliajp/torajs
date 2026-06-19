// S209 — String.repeat(undefined) returns "" per ES §22.1.3.17
// step 1: ToIntegerOrInfinity(undefined) = 0 → repeat count is
// 0 → empty string. Pre-fix tora declared the count arg as
// strict Number so the typed-Undefined call was rejecting at
// the strict-arity gate with "argument 0: expected Number, got
// Undefined".

console.log("abc".repeat(undefined))
console.log("xyz".repeat(undefined))
console.log("".repeat(undefined))

// Confirm existing positive-count path unchanged.
console.log("ab".repeat(3))
console.log("ab".repeat(0))
