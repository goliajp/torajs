// V3-18 m1.a — JS spec §13.15.3 ToNumber coercion for `+`.
// Boolean and Null are coerced to numeric (Bool→0/1, Null→0)
// when at least one side is non-Number. Pure Number+Number stays
// on the existing fast path.
//
// Foundational substrate for the test262 push — every JS-shape
// arithmetic test (no type annotations) eventually hits one of
// these coercion edges. matches bun byte-for-byte.

console.log(1 + true)
console.log(true + 1)
console.log(true + true)
console.log(1 + null)
console.log(null + 1)
console.log(null + null)
console.log(false + 0)
console.log(true + false)
console.log(0 + false + true)
