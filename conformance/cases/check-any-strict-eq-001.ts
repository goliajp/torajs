// P0.3 — `===` / `!==` between Any operand and concrete (or
// two Any operands) per JS spec §7.2.13 StrictEqualityComparison.
// Pre-fix the BinOp Eq/Neq lower had a static "different family →
// false" fold that fired for `Any === Number` and similar mixes
// because Any is in the pointerish family while concrete numerics
// aren't. Result: every `let x: any = 5; x === 5` returned false
// statically — the canonical TS test262 assert pattern was dead
// on arrival.
//
// Implementation:
// * runtime_str.c: __torajs_any_payload_eq inner switches on the
//   tag (I64/Bool/Null → integer compare, F64 → IEEE compare via
//   bitcast union, Str → str_eq on STR/STR pair, other heap →
//   pointer identity). +0 === -0 holds at IEEE level. NaN !==
//   NaN naturally.
// * runtime_str.c: __torajs_any_any_strict_eq(l, r) for Any ===
//   Any — load both tags, compare, then defer to payload_eq.
// * runtime_str.c: __torajs_any_strict_eq(box, rhs_tag, rhs_value)
//   for Any === concrete. Caller packs the concrete operand at
//   the SSA layer (compile-time tag, bitcast/zext value-as-i64);
//   helper avoids a fresh Any-box alloc per compare.
// * ssa_lower: BinOp Eq/Neq detects Type::Any in either operand
//   and routes accordingly. !== flips the result via Xor with
//   ConstBool(true). Other typed-tier BinOp paths unchanged.

// Any vs concrete primitive — same value.
let n: any = 42
console.log(n === 42)                        // true
console.log(n !== 42)                        // false

// Any vs concrete — different value.
console.log(n === 99)                        // false
console.log(n !== 99)                        // true

// Any vs concrete — different type (number vs string).
console.log(n === "42")                      // false

// Any vs Null.
console.log(n === null)                      // false

let nullable: any = null
console.log(nullable === null)               // true
console.log(nullable === 0)                  // false (null !== 0 strictly)

// Symmetric — concrete vs Any (LHS swap).
console.log(42 === n)                        // true
console.log("42" === n)                      // false

// Any vs Any — same primitive.
let a: any = 5
let b: any = 5
console.log(a === b)                         // true

// Any vs Any — different tags.
let c: any = "5"
console.log(a === c)                         // false

// Any-boxed strings — payload byte-eq via str_eq.
let s1: any = "hello"
let s2: any = "hello"
let s3: any = "world"
console.log(s1 === s2)                       // true
console.log(s1 === s3)                       // false
console.log(s1 === "hello")                  // true (Any vs concrete Str)
console.log("hello" === s1)                  // true (symmetric)

// Any-boxed booleans.
let bt: any = true
let bf: any = false
console.log(bt === true)                     // true
console.log(bf === false)                    // true
console.log(bt === bf)                       // false

// Any-boxed heap pointer identity.
let arr: any = [1, 2, 3]
let arr2: any = arr   // same underlying heap
console.log(arr === arr2)                    // true (pointer identity)
