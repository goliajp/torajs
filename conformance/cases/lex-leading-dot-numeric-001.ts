// P0.10 (lexer extension) — leading-dot numeric literals per
// ES spec §12.9.3 NumericLiteral. Pre-fix tora's lexer always
// emitted Token::Dot for `.` followed by a digit, leaving the
// parser to bail with 'expected expression, got Dot'. Test262's
// language/literals/numeric/S7.8.3_A2.* suite uses these
// pervasively (~20+ cases).
//
// Implementation:
// * lexer.rs: when the byte after `.` is an ASCII digit, treat
//   the whole sequence as a numeric literal — synthesize the
//   leading "0", consume the fractional digits (with `_`
//   separators per ES2021 numeric-separator), then optionally
//   the exponent `[eE][+-]?DIGITS`. Emit Token::Number(f64).
// * Three-dot spread (`...`) check happens first so the existing
//   destructuring / spread surface stays unchanged.
// * Regular `.` (member access, decimal already-parsed value)
//   stays as Token::Dot.

console.log(.5)                              // 0.5
console.log(.123)                            // 0.123
console.log(.0)                              // 0
console.log(.5e2)                            // 50
console.log(.5e-2)                           // 0.005
console.log(.5E3)                            // 500
console.log(.1234567)                        // 0.1234567

// Numeric separator inside leading-dot fractional part.
console.log(.1_000)                          // 0.1
console.log(.123_456)                        // 0.123456

// Regression: regular member access still parses.
let o = { x: 42, y: 99 }
console.log(o.x)                             // 42
console.log(o.y)                             // 99

// Regression: standard 0.5-form numeric literal still parses.
console.log(0.5)                             // 0.5
console.log(3.14)                            // 3.14

// Regression: spread / rest operator unaffected.
let xs = [1, 2, 3]
let ys = [0, ...xs]
console.log(ys.length)                       // 4
