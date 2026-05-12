// V3-18 wedge — Boolean.prototype.toString / .valueOf. Per
// JS spec §20.3.3.2 / §20.3.3.3:
//   true.toString()  → "true"
//   false.toString() → "false"
//   b.valueOf()      → b (identity)
// Pre-fix tora's check.rs rejected with 'no member .toString
// on type Boolean'. The dispatch arm now routes Bool receivers
// through __torajs_bool_to_str (the same runtime intrinsic
// used by Number-to-String coercion in `+`).

console.log(true.toString())           // true
console.log(false.toString())          // false

let b = true
console.log(b.toString())              // true

let b2 = false
console.log(b2.valueOf())              // false

// Inline from a comparison expr.
console.log((3 < 4).toString())        // true
console.log((1 == 1).toString())       // true
console.log((1 > 2).toString())        // false

// As a chainable conversion before string concat.
console.log("yes? " + true.toString())  // yes? true
