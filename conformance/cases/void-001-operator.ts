// V3-18 m1.h.30 — `void <expr>` evaluates expr (for side effects)
// then yields `undefined`. Per JS spec §13.5.2. Pre-fix the
// parser bailed with "expected `)`, got Number" on `void 0`.
//
// Tora doesn't yet have a separate undefined sentinel distinct
// from null, so the pragmatic desugar is
// `Sequence { left: <expr>, right: String("undefined") }`. The
// surface behavior matches bun for the common shapes
// (console.log, string concat). `typeof void X` differs because
// the operand is a Str at runtime — that lands when the implicit-
// any substrate gives us a real Type::Undefined.

console.log(void 0)
console.log(void 5)
console.log(void "literal")

let n = 42
console.log(void n)

// Side-effect path: expr is evaluated even though result is
// always "undefined". Use a side effect that's local to the
// `void` expression itself.
let s = "x"
console.log(void (s + "!"))
console.log(s)
