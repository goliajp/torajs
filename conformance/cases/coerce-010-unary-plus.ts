// V3-18 m1.h.4 — JS spec §13.5.4 unary `+x` calls ToNumber(x).
// Common test262 idiom for explicit numeric coercion.
//   +5    → 5
//   +true → 1
//   +false → 0
//   +null → 0
//   +0    → 0
//   +(-3) → -3
console.log(+5)
console.log(+true)
console.log(+false)
console.log(+null)
console.log(+0)
console.log(+(-3))
