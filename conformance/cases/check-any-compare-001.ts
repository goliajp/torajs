// P0.8 — Any-aware ordering compare (`<` `<=` `>` `>=`) per
// JS spec §7.2.13 IsLessThan and §13.10. Pre-fix the ordering
// arms only matched concrete-type combinations and rejected any
// Any participation with 'ordering comparison requires number
// or bigint operands, got Any and Number'. Test262 uses these
// pervasively in numeric range assertions and string-ordering
// invariants.
//
// Implementation:
// * runtime_str.c: __torajs_any_compare(int64_t op, int64_t lt,
//   int64_t lv, int64_t rt, int64_t rv) — both sides packed as
//   (tag, value-as-i64), op code 0=Lt, 1=Le, 2=Gt, 3=Ge.
//   Per spec §7.2.13: if both operands are HEAP/Str do
//   lexicographic memcmp (correct byte-order for ASCII; full
//   UTF-16 code-unit semantics is a follow-up); otherwise
//   ToNumber both and compare in IEEE 754. NaN comparisons
//   all return false per spec.
// * check.rs: Lt/Le/Gt/Ge accept Any in either operand,
//   return Type::Boolean.
// * ssa_lower: BinOp dispatch generalised to also handle
//   Lt/Le/Gt/Ge alongside Add and Sub/Mul/Div/Mod. Same pack
//   helper packs each operand. Compare returns Bool directly
//   (not Any) since the result is always boolean.

let n: any = 5
console.log(n < 10)                          // true
console.log(n > 10)                          // false
console.log(n <= 5)                          // true
console.log(n >= 5)                          // true
console.log(n < 5)                           // false

// String ToNumber via Any.
let sn: any = "5"
console.log(sn < 10)                         // true (5 < 10)
console.log(sn > 4)                          // true
console.log(sn <= 5)                         // true

// Both operands String → lex compare.
let s1: any = "apple"
let s2: any = "banana"
console.log(s1 < s2)                         // true
console.log(s2 > s1)                         // true
console.log(s1 < s1)                         // false
console.log(s1 <= s1)                        // true
console.log(s1 === s1)                       // true

// Lex with non-ascii ordering — ASCII byte values.
console.log("a" < "5")                       // false (ASCII a=0x61 > 5=0x35)
console.log("Z" < "a")                       // true (Z=0x5a < a=0x61)

// Any + Any compare both numbers.
let a: any = 3
let b: any = 7
console.log(a < b)                           // true
console.log(a >= b)                          // false
console.log(a <= b)                          // true

// Boolean ToNumber.
let bt: any = true
let bf: any = false
console.log(bf < bt)                         // true (0 < 1)
console.log(bt > 0)                          // true
console.log(bf >= 0)                         // true (0 >= 0)

// Null ToNumber → 0.
let nl: any = null
console.log(nl < 1)                          // true (0 < 1)
console.log(nl <= 0)                         // true

// NaN comparisons all false.
let nan: any = 0 / 0
console.log(nan < 5)                         // false
console.log(nan > 5)                         // false
console.log(nan <= 5)                        // false
console.log(nan >= 5)                        // false
