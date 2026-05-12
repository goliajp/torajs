// V3-18 Phase D — `undefined` distinct from `null` for the most
// common observation paths:
//   console.log(undefined) → "undefined"  (was "0")
//   undefined === null     → false        (was true)
//   undefined === undefined → true
//   typeof undefined       → "undefined"  (already worked)
//
// Tora doesn't yet have a distinct Type::Undefined sentinel — both
// `null` and `undefined` collapse to ConstPtrNull at the runtime
// layer. The fix is syntactic: detect the literal `Ident("undefined")`
// shape at lower time and emit the spec-correct path. Real
// Type::Undefined ships with the dynamic-substrate phase.

console.log(undefined)                  // undefined
console.log(undefined === null)         // false
console.log(undefined === undefined)    // true
console.log(undefined !== null)         // true
console.log(undefined !== undefined)    // false
console.log(typeof undefined)           // undefined

// Null cases still work (no regression).
console.log(null === null)              // true
console.log(null !== null)              // false

// Cross-comparisons fold at compile time.
console.log(null === undefined)         // false
console.log(null !== undefined)         // true
