// V3-18 m1.h.2 — JS spec §13.5.7 logical NOT calls ToBoolean
// on its operand. Tora previously rejected `!1` / `!"x"` etc
// with "boolean operand required". Now coerces via
// coerce_to_bool then xors with true.
console.log(!1)
console.log(!0)
console.log(!"hi")
console.log(!"")
console.log(!null)
console.log(!!0)
console.log(!!"a")
console.log(!!null)
