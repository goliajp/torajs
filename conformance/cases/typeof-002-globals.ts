// V3-18 m1.h.20 — typeof on known JS globals + on
// `<namespace>.<member>` resolves to the spec literal at compile
// time. Without this, lowering tried to treat the global Ident
// (or its outer object) as a SSA local and bailed with
// "ssa-lower: unknown ident `console`". test262 uses the typeof
// feature-detection idiom pervasively
// (`typeof BigInt === "function"`, `typeof Math.abs === "function"`).

console.log(typeof undefined)
console.log(typeof Math)
console.log(typeof JSON)
console.log(typeof console)
console.log(typeof globalThis)

// Constructors → "function"
console.log(typeof Number)
console.log(typeof String)
console.log(typeof Boolean)
console.log(typeof Symbol)
console.log(typeof Date)
console.log(typeof Array)
console.log(typeof Object)
console.log(typeof RegExp)
console.log(typeof Error)
console.log(typeof Promise)
console.log(typeof Map)
console.log(typeof Set)
console.log(typeof BigInt)

// Top-level coercion functions → "function"
console.log(typeof parseInt)
console.log(typeof parseFloat)
console.log(typeof isNaN)
console.log(typeof isFinite)

// Member dispatch.
console.log(typeof console.log)
console.log(typeof Math.abs)
console.log(typeof JSON.stringify)
console.log(typeof Math.PI)
console.log(typeof Math.E)
console.log(typeof Number.MAX_VALUE)
console.log(typeof Number.MAX_SAFE_INTEGER)
