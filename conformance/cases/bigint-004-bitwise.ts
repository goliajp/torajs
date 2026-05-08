// V3-02 (T-25 follow-up) — BigInt bitwise: & | ^ ~ << >>.
// Two's-complement simulation over sign-magnitude storage.
// `>>>` on BigInt is a TypeError per spec — caught at typecheck,
// not exercised here.

// Positive-only AND/OR/XOR — direct magnitude bitops.
console.log(0xffn & 0x0fn)
console.log(0xf0n | 0x0fn)
console.log(0xffn ^ 0x0fn)

// Unary `~x` ≡ `-x - 1n`.
console.log(~5n)
console.log(~(-5n))
console.log(~0n)

// Mixed-sign AND/OR/XOR — exercise the 4 sign-case dispatch.
console.log((-5n) & 3n)
console.log((-5n) | 3n)
console.log((-5n) ^ 3n)
console.log((-5n) & (-3n))
console.log((-5n) | (-3n))
console.log((-5n) ^ (-3n))

// Shifts — << grows magnitude across limb boundaries; >> for
// negative floors toward -∞.
console.log(1n << 64n)
console.log(1n << 128n)
console.log(0xffffn >> 4n)
console.log((-1n) >> 1n)
console.log((-7n) >> 1n)
console.log(8n >> 64n)
console.log(8n >> 1000n)              // beyond magnitude: truncates to 0
console.log(7n << -1n)                // negative shift = opposite direction
console.log(7n >> -1n)                // ≡ 7n << 1n = 14n

// Big-magnitude bitops — multi-limb path.
let big: bigint = 0xffffffffffffffffffffffffffffffffn   // 16 bytes of 1s
let mask: bigint = 0x0000000000000000ffffffffffffffffn
console.log(big & mask)
console.log(big >> 64n)
console.log(big | (1n << 200n))
