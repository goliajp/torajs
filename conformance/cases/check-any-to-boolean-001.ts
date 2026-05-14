// P0.4 — ToBoolean(Any) per JS spec §7.1.2. Pre-fix the
// coerce_to_bool fallback for heap-pointer Any operand returned
// `ptr != null` which is true for any non-NULL Any-box —
// regardless of the boxed payload. Result: `let x: any = 0;
// if (x) { ... }` always entered the truthy branch even though
// the boxed value is 0.
//
// Implementation:
// * runtime_str.c: __torajs_any_to_bool((const void *box))
//   reads the tag and payload, applies spec rules:
//     ANY_NULL          → false
//     ANY_BOOL          → value (0/1)
//     ANY_I64           → value != 0
//     ANY_F64           → value != 0 AND not NaN  (spec §7.1.2)
//     ANY_HEAP / Str    → length > 0  (empty string is falsy)
//     ANY_HEAP / other  → true        (objects always truthy)
//     NULL box ptr      → false       (defensive)
// * ssa_lower: coerce_to_bool gains a Type::Any arm that routes
//   through the new helper. Every cond site that flowed an
//   Any-typed operand through coerce_to_bool now picks up the
//   spec-correct truthiness.
//
// Effect: `if (x)` / `!x` / `x ? a : b` / `x && y` / `x || y` /
// `Boolean(x)` all return the spec-mandated result for Any
// operands. test262's assert.sameValue / assert.notSameValue
// patterns rely on this — together with P0.3 (===) the assert
// harness for any-typed values starts passing end-to-end.

// Number Any.
let n5: any = 5
if (n5) console.log("n5 truthy"); else console.log("n5 falsy")  // truthy

let n0: any = 0
if (n0) console.log("n0 truthy"); else console.log("n0 falsy")  // falsy

let nn: any = -3.14
if (nn) console.log("nn truthy"); else console.log("nn falsy")  // truthy

let nf: any = 0.0
if (nf) console.log("nf truthy"); else console.log("nf falsy")  // falsy

// String Any.
let sa: any = "hello"
if (sa) console.log("sa truthy"); else console.log("sa falsy")  // truthy

let se: any = ""
if (se) console.log("se truthy"); else console.log("se falsy")  // falsy

// Boolean Any.
let bt: any = true
if (bt) console.log("bt truthy"); else console.log("bt falsy")  // truthy

let bf: any = false
if (bf) console.log("bf truthy"); else console.log("bf falsy")  // falsy

// Null Any.
let nl: any = null
if (nl) console.log("nl truthy"); else console.log("nl falsy")  // falsy

// Heap object Any — always truthy regardless of payload shape.
let arr: any = [1, 2, 3]
if (arr) console.log("arr truthy"); else console.log("arr falsy")  // truthy

// `!any` / negation.
console.log(!n5)                             // false
console.log(!n0)                             // true
console.log(!sa)                             // false
console.log(!se)                             // true
console.log(!nl)                             // true
console.log(!arr)                            // false

// Ternary picks correctly.
console.log(n5 ? "yes" : "no")               // yes
console.log(n0 ? "yes" : "no")               // no
console.log(sa ? "yes" : "no")               // yes
