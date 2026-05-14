// P0.6 — `+` with Any operand per JS spec §13.15.3
// ApplyStringOrNumericBinaryOperator. Pre-fix the BinOp Add
// arm in check.rs only matched concrete-type combinations and
// rejected any Any participation with '`+` requires matching
// number/string/bigint operands or string+number, got
// String and Any' / similar. Test262 assert error messages
// pervasively concat string prefixes with the actual mis-
// matched value via `+` ("got: " + actual), so blocking Any
// in `+` blocks every assertion-failure path on Any-typed
// values.
//
// Implementation:
// * runtime_str.c: __torajs_any_add(int64_t lt, int64_t lv,
//     int64_t rt, int64_t rv) takes both operands packed as
//   (tag, value-as-i64) tuples. Per spec ToPrimitive(default):
//     - either side is HEAP/Str → concat both via ToString
//     - else → ToNumber both, sum as f64 (i64-encode if
//       integer-valued in i64 range — keeps Number printing
//       clean without trailing .0).
//   Returns a fresh Any-box.
// * runtime_str.c: __torajs_any_to_str helper — handles all
//   primitive tags via the existing _to_str family
//   (i64_to_str / f64_to_str / bool_to_str / null_to_str).
//   For Obj/Arr/Closure heap types substitutes "[object]"
//   placeholder until P3 ships full pretty-print.
// * check.rs: Add accepts Any in either operand position
//   when no other concrete arm matches; result is Type::Any.
// * ssa_lower: BinOp Add with Any operand packs both sides
//   (Any: load tag at offset 8 + value at offset 16; concrete:
//   compile-time tag + bitcast/zext value-as-i64), calls
//   the helper. Result is a fresh Any-box.
//
// Other arithmetic (- * / %) on Any operand is a follow-up
// substrate item — those always ToNumber both sides, no
// String concat path. The plumbing here generalises to those
// once their helpers land.

// String + Any-Number → String concat.
let n: any = 5
console.log("n=" + n)                        // n=5
console.log("count: " + (n + 3))             // count: 8

// Any-Number + concrete number → Number.
console.log(n + 3)                           // 8
console.log(3 + n)                           // 8

// Any-Number + concrete string → String concat.
console.log(n + "a")                         // 5a

// Any-String + concrete string → String concat.
let s: any = "hi"
console.log(s + "!")                         // hi!
console.log("[" + s + "]")                   // [hi]

// Concrete number + Any-String → String concat (per spec).
console.log(1 + s)                           // 1hi

// Any + Any — number-number.
let a: any = 1
let b: any = 2
console.log(a + b)                           // 3
console.log(a + b + 3)                       // 6

// Any + Any — string-string.
let s1: any = "hello"
let s2: any = "world"
console.log(s1 + " " + s2)                   // hello world

// Any + Any — number + string (any-tag mix).
console.log(a + s)                           // 1hi
console.log(s + a)                           // hi1

// Bool ToNumber via Any.
let bo: any = true
console.log(bo + 1)                          // 2 (true → 1)
console.log(bo + " yes")                     // true yes

// Null ToNumber / ToString via Any.
let nl: any = null
console.log(nl + 5)                          // 5  (null → 0)
console.log(nl + " hi")                      // null hi
