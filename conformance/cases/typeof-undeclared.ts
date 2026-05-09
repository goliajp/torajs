// V3-18 m1.h.3 — JS spec §13.5.3 typeof on an unresolved
// Reference returns "undefined" without throwing ReferenceError.
// Used pervasively in test262 for feature detection
// (`typeof BigInt === "function"` etc).
console.log(typeof undeclared)
console.log(typeof someThing)
console.log(typeof __nonexistent_global)
let x = 5
console.log(typeof x)
console.log(typeof "hello")
console.log(typeof 42)
