// T-25 (v0.7) — BigInt literal + arithmetic + comparison.

// Decimal literal beyond i64 range (>2^63).
let a: bigint = 123456789012345678901234567890n
let b: bigint = 987654321098765432109876543210n
console.log(a)
console.log(b)

// Sum + difference + product.
console.log(a + b)
console.log(b - a)
console.log(a * 2n)
console.log(b - b)         // canonical zero
console.log(0n - a)        // sign flip via subtraction

// Comparison.
console.log(a < b)
console.log(a > b)
console.log(a === a)
console.log(a !== b)
console.log(a + b === b + a)

// Hex literal.
let h: bigint = 0xffffffffffffffffn
console.log(h)
console.log(h + 1n)

// typeof returns "bigint" per spec.
console.log(typeof a)
