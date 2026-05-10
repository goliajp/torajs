// V3-18 m1.h.8 — JS spec callable coercion ctors. Each takes 0
// or 1 arg, NEVER `new`-called (the wrapper-object form is
// deferred — see V3-18 m1 follow-ups).
//   Number(value)  → ToNumber(value) per §21.1.1.1
//   String(value)  → ToString(value) per §22.1.1.1
//   Boolean(value) → ToBoolean(value) per §20.3.1.1
// Number(string) coercion (parse-or-NaN) is m1.h.9 follow-up.
console.log(Number(true))
console.log(Number(false))
console.log(Number(null))

console.log(String(42))
console.log(String(true))
console.log(String(null))

console.log(Boolean(0))
console.log(Boolean(1))
console.log(Boolean(""))
console.log(Boolean("a"))
console.log(Boolean(null))
