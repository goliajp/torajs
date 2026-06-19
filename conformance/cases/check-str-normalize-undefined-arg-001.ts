// S208 — String.normalize(undefined) routes to NFC default per
// ES §22.1.3.13 step 1: when `form` is undefined, default to
// "NFC". The 0-arg form was already covered; explicit undefined
// was rejecting at the strict-arity gate with
// "String.normalize arg must be string, got Undefined".

console.log("hello".normalize())
console.log("hello".normalize(undefined))
console.log("hello".normalize("NFC"))

// Confirm the identity path holds for ASCII chars (tora's
// byte-Str path is NFC by construction).
console.log("abc".normalize())
console.log("abc".normalize(undefined))

// Empty haystack edge case.
console.log("".normalize())
console.log("".normalize(undefined))
