// T-25 follow-up — BigInt division + modulo. Spec rules:
//   - / truncates toward zero (drops fractional part).
//   - / sign = lhs.sign XOR rhs.sign.
//   - % sign = lhs.sign (so a / b * b + a % b === a always holds).
//   - / 0n  → RangeError (not exercised here; a separate fixture
//     would need a try/catch wrapper bun's RangeError text).

console.log(7n / 3n)
console.log(-7n / 3n)
console.log(7n / -3n)
console.log(-7n / -3n)

console.log(7n % 3n)
console.log(-7n % 3n)
console.log(7n % -3n)
console.log(-7n % -3n)

// Exact division — no remainder.
console.log(20n / 4n)
console.log(20n % 4n)

// Zero dividend.
console.log(0n / 5n)
console.log(0n % 5n)

// Multi-limb divmod identity: (a / b) * b + (a % b) === a.
let a: bigint = 123456789012345678901234567890n
let b: bigint = 987654321n
let q = a / b
let r = a % b
console.log(q)
console.log(r)
console.log(q * b + r === a)

// 100-digit divisor exercises the bit-by-bit long-division path.
let big_a: bigint = 99999999999999999999999999999999999999999999999999n
let big_b: bigint = 12345n
console.log(big_a / big_b)
console.log(big_a % big_b)
console.log((big_a / big_b) * big_b + (big_a % big_b) === big_a)
