// V3-18 m1.h.30 — `void <expr>` evaluates expr (for side effects)
// then yields `undefined`. Per JS spec §13.5.2. Pre-fix the
// parser bailed with "expected `)`, got Number" on `void 0`.
//
// RC-4 F1b-1: desugars to `Sequence { left: <expr>, right:
// Ident("undefined") }` — `void 0` is the same value as the
// `undefined` Ident (Type::Undefined / ConstPtrNull), not the
// former String("undefined") stand-in, so identity compares
// against void 0 behave per spec.

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

// RC-4 F1b-1 — identity semantics: `void 0` compares as the
// undefined value, never as the string "undefined".
console.log("undefined" === void 0)
console.log("undefined" !== void 0)
console.log(void 0 === undefined)
let m = "ab5".match(/(a)(q)?/)
console.log(m[2] === void 0)
console.log(m[2] !== void 0)
console.log(m[1] === void 0)
