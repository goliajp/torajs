// V3-18 m1.e — JS spec §13.12 ToInt32 coercion for bitwise ops
// (& | ^ << >> >>>) with Bool / Null operands. Same per-side
// coerce-to-i64 path as the arith ops; the bitwise ops then
// operate as i32 per spec.
console.log(true & 5)
console.log(true | 0)
console.log(true ^ 1)
console.log(null & 5)
console.log(null | 7)
console.log(null ^ 3)
console.log(true << 2)
console.log(true >> 0)
console.log(8 >>> true)
console.log(false << 4)
