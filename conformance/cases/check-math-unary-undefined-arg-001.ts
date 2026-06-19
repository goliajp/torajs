// S227 — `Math.<unary>(undefined)` for the NaN-propagating unary
// methods per ES §21.3.2.* step 1: `ToNumber(undefined) = NaN`,
// then the method returns NaN unchanged. `Math.clz32(undefined)`
// is the lone exception — its ToUint32(undefined)=0 path returns
// 32. Mirror of the 0-arg fold S203 already supports.

console.log(Math.sqrt(undefined))
console.log(Math.abs(undefined))
console.log(Math.floor(undefined))
console.log(Math.ceil(undefined))
console.log(Math.round(undefined))
console.log(Math.trunc(undefined))
console.log(Math.sign(undefined))
console.log(Math.log(undefined))
console.log(Math.exp(undefined))
console.log(Math.sin(undefined))
console.log(Math.cos(undefined))
console.log(Math.tan(undefined))
console.log(Math.asin(undefined))
console.log(Math.acos(undefined))
console.log(Math.atan(undefined))
console.log(Math.log2(undefined))
console.log(Math.log10(undefined))
console.log(Math.cbrt(undefined))
console.log(Math.sinh(undefined))
console.log(Math.cosh(undefined))
console.log(Math.tanh(undefined))
console.log(Math.asinh(undefined))
console.log(Math.acosh(undefined))
console.log(Math.atanh(undefined))
console.log(Math.expm1(undefined))
console.log(Math.log1p(undefined))
console.log(Math.fround(undefined))
console.log(Math.f16round(undefined))

// clz32 special: ToUint32(undefined) = 0, clz32(0) = 32.
console.log(Math.clz32(undefined))

// non-undef regression — real numeric arg still works.
console.log(Math.sqrt(16))
console.log(Math.abs(-5))
console.log(Math.floor(3.7))
console.log(Math.clz32(1))

// 0-arg S203 regression.
console.log(Math.sqrt())
console.log(Math.clz32())
