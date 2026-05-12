// V3-18 wedge — `==` / `!=` (loose equality) on Str/Str routes
// to str_eq, matching the existing `===` / `!==` (strict) path.
// Per JS spec §7.2.13 IsLooselyEqual: when both operands ToPrimitive
// to String, the comparison is content-equal. Pre-fix tora's
// str_eq dispatch only matched AstBinOp::Eq / Neq (strict);
// LooseEq / LooseNeq fell through to the generic numeric / pointer
// comparison and silently returned false for content-equal strings.

console.log("a" + "b" == "ab")          // true (was false)
console.log("a" + "b" === "ab")         // true (no regression)
console.log(("a" + "b") == "ab")        // true
console.log("ab" == "ab")               // true
console.log("ab" != "cd")               // true
console.log("ab" != "ab")               // false

// Substr operands.
let parts = "hello,world".split(",")
console.log(parts[0] == "hello")        // true
console.log(parts[0] != "hello")        // false
