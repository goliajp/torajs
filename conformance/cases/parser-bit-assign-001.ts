// V3-18 wedge — bitwise compound assignments per JS spec
// §13.15.2:
//   x |= y    — x = x | y    (bit-OR)
//   x ^= y    — x = x ^ y    (bit-XOR)
//   x &= y    — x = x & y    (bit-AND)
//   x <<= y   — x = x << y   (left shift)
//   x >>= y   — x = x >> y   (signed right shift)
//   x >>>= y  — x = x >>> y  (unsigned right shift)
// Pre-fix tora's parser bailed at the second token because
// `|`, `^`, `&`, `<<`, `>>`, `>>>` were consumed first by
// their precedence levels, leaving a bare `=` start-of-expr.
//
// Implementation: parse_assign peeks the two-token sequence
// (Pipe Eq, Caret Eq, Amp Eq, ShlShl Eq, ShrShr Eq, ShrShrShr
// Eq); parse_bit_or / parse_bit_xor / parse_bit_and / parse_shift
// decline to consume their op when an `=` follows so the
// sequence falls through to parse_assign. The rhs is a regular
// BinOp; the outer wrap is Expr::Assign — same shape as `+=`.

let x = 0b1010
x <<= 2
console.log(x)                         // 40

x >>= 1
console.log(x)                         // 20

x |= 0b11
console.log(x)                         // 23

x ^= 0b101
console.log(x)                         // 18

x &= 0b110
console.log(x)                         // 2

// Member-target form (lhs single-eval).
type Flag = { v: number }
let f: Flag = { v: 0b001 }
f.v |= 0b010
console.log(f.v)                       // 3

// Unsigned shift assign (>>>=).
let n = 0xff000000 | 0
n >>>= 4
console.log(n)                         // 267386880

// Mixed with regular `=`, `+=` in the same flow.
function step(v: number): number {
  v += 1
  v |= 0b1
  return v
}
console.log(step(8))                   // 11
console.log(step(0))                   // 1
