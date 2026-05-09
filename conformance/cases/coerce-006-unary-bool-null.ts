// V3-18 m1.f — JS spec §13.5.5/§13.5.6 ToNumber/ToInt32 for
// unary `-` and `~` with Boolean / Null operands.
//   -true  → -1
//   -false → -0   (IEEE 754 sign preserved by routing via f64)
//   -null  → -0
//   ~true  → -2
//   ~false → -1
//   ~null  → -1
console.log(-true)
console.log(-false)
console.log(-null)
console.log(~true)
console.log(~false)
console.log(~null)
