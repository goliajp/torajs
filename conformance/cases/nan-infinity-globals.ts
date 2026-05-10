// V3-18 m1.h.11 — JS spec §19.1.1 NaN and §19.1.2 Infinity
// global identifiers. Both Number-typed (f64 NaN and +∞).
// Spec writes both as non-writable, non-configurable globals;
// tora lowers them to ConstF64 inline since no other code can
// shadow them at the language level.
console.log(NaN)
console.log(Infinity)
console.log(-Infinity)
console.log(NaN + 1)
console.log(Infinity * 2)
console.log(NaN === NaN)
console.log(Infinity > 1e300)
console.log(typeof NaN)
console.log(typeof Infinity)
