// V3-18 m1.h.21 — `0..toString()` form. JS spec §12.8.3 allows
// DecimalLiteral to end with a trailing `.` (no fractional digits);
// the second `.` then begins a member access.
//
// Pre-fix: lexer required `.` to be followed by a digit before
// consuming it as part of the number, so `0..toString()` lexed as
// `Number(0) . . Ident(toString)` and parser bailed with
// "expected identifier after `.`, got Dot".
//
// Now: when `.` is followed by another `.`, consume the first
// dot as the float trailing dot. Used by 20+ test262 cases
// (built-ins/Number/prototype/toString/numeric-literal-*) and
// the standard JS idiom for member-on-numeric-literal without
// parentheses.

console.log(0..toString())
console.log(1..toString())
console.log(255..toString(16))
console.log(255..toString(8))
console.log(7..toString(2))

console.log(NaN.toString())
console.log(Infinity.toString())
console.log((-3.14).toString())

// Int and float forms still work — no regression.
console.log((42).toString())
console.log((3.14).toString())
