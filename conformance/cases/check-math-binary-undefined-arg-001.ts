// S228 — `Math.{pow, atan2, imul, min, max}` with explicit
// `undefined` arg(s) per ES §21.3.2.{5,19,21,24,25,26}:
//   pow / atan2 / min / max:  ToNumber(undefined) = NaN → NaN
//                             propagates through the operation.
//   imul:                     ToUint32(undefined) = 0 → 32-bit
//                             multiply with 0 returns 0.

console.log("=== Math.pow ===")
console.log(Math.pow(undefined, 2))
console.log(Math.pow(2, undefined))
console.log(Math.pow(undefined, undefined))

console.log("=== Math.atan2 ===")
console.log(Math.atan2(undefined, 1))
console.log(Math.atan2(1, undefined))
console.log(Math.atan2(undefined, undefined))

console.log("=== Math.imul ===")
console.log(Math.imul(undefined, 5))
console.log(Math.imul(5, undefined))
console.log(Math.imul(undefined, undefined))

console.log("=== Math.min ===")
console.log(Math.min(undefined, 5))
console.log(Math.min(5, undefined))
console.log(Math.min(undefined))
console.log(Math.min(1, 2, undefined, 4))

console.log("=== Math.max ===")
console.log(Math.max(undefined, 5))
console.log(Math.max(5, undefined))
console.log(Math.max(undefined))
console.log(Math.max(1, 2, undefined, 4))

// non-undef regression — numeric path still works.
console.log("=== non-undef ===")
console.log(Math.pow(2, 10))
console.log(Math.atan2(1, 1))
console.log(Math.imul(3, 4))
console.log(Math.min(5, 3, 8))
console.log(Math.max(5, 3, 8))
