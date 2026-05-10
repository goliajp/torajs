// V3-18 m1.h.32 — `NaN !== NaN` must be `true` per JS spec
// §7.2.16. Pre-fix tora's f64 path lowered AstBinOp::Neq to
// `FCmp::One` (ordered-not-equal), which returns false when
// either operand is NaN. The correct shape is `FCmp::Une`
// (unordered-or-not-equal): true if either side is NaN OR the
// values differ.
//
// `NaN === NaN` correctly stays false (Oeq treats NaN as
// unordered-and-not-equal). The fix is asymmetric: only the !=
// path needs the unordered variant.

console.log(NaN === NaN)         // false
console.log(NaN !== NaN)         // true (was false pre-fix)
console.log(NaN == NaN)          // false
console.log(NaN != NaN)          // true (was false pre-fix)

console.log(0 === 0)             // true
console.log(0 !== 0)             // false
console.log(1.5 !== 2.5)         // true (normal case still works)
console.log(1.5 !== 1.5)         // false
console.log(1.5 === 1.5)         // true

// Mixed NaN comparisons.
let x = NaN
console.log(x !== 5)              // true
console.log(x !== x)              // true
