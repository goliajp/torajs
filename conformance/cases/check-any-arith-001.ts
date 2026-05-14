// P0.7 — Any-aware `-` `*` `/` `%` per JS spec §13.6 / §13.7 /
// §13.8 / §13.9. Pre-fix the BinOp Sub/Mul/Div/Mod arms only
// matched concrete-type combinations and rejected any Any
// participation with 'arithmetic requires number or bigint
// operands, got Any and Number'. Common in test262 expressions
// (e.g. `assert.sameValue(value - 1, expected)`) and in
// numeric-mixing assertion-failure messages.
//
// Implementation:
// * runtime_str.c: __torajs_any_arith(int64_t op, int64_t lt,
//   int64_t lv, int64_t rt, int64_t rv) takes both operands
//   packed as (tag, value-as-i64) and an op code (0=Sub,
//   1=Mul, 2=Div, 3=Mod). Per spec §7.1.4 ToNumber both sides
//   then perform the IEEE 754 op. Result is i64-encoded when
//   both inputs are i64-shaped AND result is integer-valued in
//   i64 range (preserves clean printing without trailing .0);
//   otherwise f64-encoded. Division always returns f64 even
//   for integer ops (spec §13.8: 7/2 === 3.5).
// * runtime_str.c: __torajs_any_to_number_inner — tag-dispatched
//   ToNumber. Null → 0, Bool → 0/1, I64 → cast, F64 → bitcast,
//   HEAP/Str → strtod via existing __torajs_str_to_number,
//   HEAP/other → NaN.
// * check.rs: Sub/Mul/Div/Mod accept Any in either operand,
//   return Type::Any.
// * ssa_lower: BinOp Add path generalised to also handle Sub/
//   Mul/Div/Mod. Same pack helper packs each operand as (tag,
//   value-as-i64) — Any operand: load tag at offset 8 + value
//   at offset 16; concrete: compile-time tag + bitcast/zext.
//   Then dispatch: Add → any_add; others → any_arith with op
//   code.

let n: any = 10
console.log(n - 3)                           // 7
console.log(n * 2)                           // 20
console.log(n / 4)                           // 2.5
console.log(n % 3)                           // 1

// String → ToNumber (strtod).
let s: any = "5"
console.log(s - 1)                           // 4
console.log(s * 2)                           // 10

// Bool → ToNumber (0/1).
let bt: any = true
console.log(bt - 1)                          // 0
console.log(bt * 5)                          // 5

let bf: any = false
console.log(bf + 10)                         // 10
console.log(bf - 1)                          // -1

// Null → ToNumber (0).
let nl: any = null
console.log(nl + 7)                          // 7
console.log(nl - 3)                          // -3

// Any + Any arith.
let a: any = 6
let b: any = 4
console.log(a - b)                           // 2
console.log(a * b)                           // 24
console.log(a / b)                           // 1.5
console.log(a % b)                           // 2

// Mixed Any-types — string * number.
let sa: any = "3"
let na: any = 4
console.log(sa * na)                         // 12

// Concrete + Any (symmetric).
console.log(20 - n)                          // 10
console.log(2 * n)                           // 20
console.log(100 / n)                         // 10
console.log(13 % n)                          // 3
