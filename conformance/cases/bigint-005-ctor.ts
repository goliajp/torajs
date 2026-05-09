// V3-03 (T-25 follow-up) — `BigInt(value)` callable ctor.
// Three input types per spec:
//   BigInt(<bigint>)  → fresh clone with same value
//   BigInt(<string>)  → parse with auto-radix prefix detection
//                       (0x / 0o / 0b / decimal default)
//   BigInt(<number>)  → must be a finite integer Number;
//                       non-finite or non-integer → RangeError
//
// `new BigInt(...)` is a TypeError per spec — only the callable
// form is legal. We don't exercise the throw shape here (bun's
// stdout-only test would diverge on the error message text);
// the spec-strict TypeError lands with V3-18.

console.log(BigInt(42))
console.log(BigInt(-17))
console.log(BigInt(0))
console.log(BigInt(123456789))

console.log(BigInt('100'))
console.log(BigInt('-100'))
console.log(BigInt('0xff'))
console.log(BigInt('0o17'))
console.log(BigInt('0b1010'))
console.log(BigInt('123456789012345678901234567890'))

let b: bigint = 99n
console.log(BigInt(b))
console.log(BigInt(b) === b)
console.log(BigInt(b) + 1n === 100n)

// String parse handles 30-digit decimals.
console.log(BigInt('340282366920938463463374607431768211455'))
