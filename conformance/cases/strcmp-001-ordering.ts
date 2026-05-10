// V3-18 m1.h.17 — `<`, `>`, `<=`, `>=` on two String operands.
// JS spec §7.2.14: when both ToPrimitive to String, compare as
// sequences of code units (lex order). Implementation reuses the
// existing __torajs_str_locale_compare runtime helper (returns
// -1/0/1) and ICmps the result against 0 with the right predicate.
//
// Pre-fix: check.rs rejected with
// `ordering comparison requires number or bigint operands, got
// String and String`. Many test262 / annexB cases use string
// comparison directly inside asserts, so this single wedge
// unlocks a whole class of cases.
//
// Substr operands not yet wired here — when those surface in
// real workloads they'll need either substr↔str or a
// substr/substr comparator helper.
console.log("a" < "b")
console.log("z" > "a")
console.log("a" < "a")
console.log("a" <= "a")
console.log("a" >= "a")
console.log("apple" < "banana")
console.log("apple" < "apricot")
console.log("ab" < "abc")
console.log("abc" < "ab")

// Sort interaction.
let arr = ["banana", "apple", "cherry"]
arr.sort((a: string, b: string): number => a < b ? -1 : (a > b ? 1 : 0))
console.log(arr[0], arr[1], arr[2])
