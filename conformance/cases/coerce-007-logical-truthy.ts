// V3-18 m1.g — JS spec §13.13 LogicalAND/OR with truthy
// semantics for non-bool operands. `&&` returns left if falsy,
// else right. `||` returns left if truthy, else right. Result
// type is the common type when both sides match.
//   1 && 2 → 2  (1 is truthy)
//   0 && 2 → 0  (0 is falsy, return left)
//   1 || 2 → 1
//   0 || 2 → 2
//   "a" && "b" → "b"
//   ""  && "b" → ""
//   "a" || ""  → "a"
//   ""  || "b" → "b"
console.log(1 && 2)
console.log(0 && 2)
console.log(1 || 2)
console.log(0 || 2)
console.log(true && false)
console.log(false || true)
console.log("a" && "b")
console.log("" && "b")
console.log("a" || "")
console.log("" || "b")
