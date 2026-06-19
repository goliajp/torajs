// S229 — Number prototype `toString` / `toFixed` / `toExponential`
// / `toPrecision` with an explicit `undefined` arg per ES
// §21.1.3.{3,5,6,7}. An undef arg folds to the same default the
// 0-arg form already takes:
//   toString(undef)       → toString() with radix 10 (the default)
//   toFixed(undef)        → toFixed(0) per ToIntegerOrInfinity rule
//   toExponential(undef)  → spec "as few digits as necessary"
//   toPrecision(undef)    → toString(n) (precision=undef branch)

console.log("=== toString undef ===")
console.log((42).toString(undefined))
console.log((3.14).toString(undefined))
console.log((-7).toString(undefined))

console.log("=== toFixed undef ===")
console.log((3.14159).toFixed(undefined))
console.log((42).toFixed(undefined))
console.log((0.1 + 0.2).toFixed(undefined))

console.log("=== toPrecision undef ===")
console.log((3.14).toPrecision(undefined))
console.log((42).toPrecision(undefined))
console.log((123.456).toPrecision(undefined))

console.log("=== toExponential undef ===")
console.log((42).toExponential(undefined))
console.log((1234.5).toExponential(undefined))

// non-undef regression — numeric arg path still works.
console.log("=== numeric regression ===")
console.log((42).toString(16))
console.log((3.14159).toFixed(2))
console.log((3.14).toPrecision(2))
console.log((42).toExponential(2))

// 0-arg V3-18 m1.h.46 regression.
console.log("=== 0-arg regression ===")
console.log((42).toString())
console.log((3.14).toFixed())
console.log((3.14).toPrecision())
console.log((42).toExponential())
