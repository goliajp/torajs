// P0.10 (lexer extension) — binary / octal BigInt literals
// `0b...n` and `0o...n` per ES spec §12.9.3 NumericLiteral.
// Pre-fix tora's lexer accepted the binary / octal lexeme but
// only the hex path checked for the trailing `n` BigInt suffix;
// `0b101n` parsed as a Number `5` followed by an unattached
// Ident `n`, then the parser bailed at the `n`. Test262's
// language/literals/bigint/numeric-separators/numeric-separator-
// literal-bil-* and -oil-* cases hit this (~10+ cases).
//
// Implementation:
// * lexer.rs binary path: when the digit body is followed by
//   `n`, parse as u64 (binary radix=2), convert to a decimal
//   string, emit Token::BigInt { digits, radix: 10 }. Reuses
//   the existing bigint_from_decimal runtime helper — the
//   binary-aware base-conversion happens at lex time.
// * lexer.rs octal path: same shape with u64 radix=8.
// * Subset constraint: values must fit in u64 (the lex-time
//   pre-convert uses u64). Larger values would need
//   arbitrary-precision base conversion (separate substrate
//   item once we hit a real test262 case with values past
//   2^64).

// Binary BigInt with separator.
console.log(0b0_1n)                          // 1n
console.log(0B0_1n)                          // 1n
console.log(0b101_010n)                      // 42n
console.log(0b1111_1111n)                    // 255n

// Octal BigInt with separator.
console.log(0o7_7n)                          // 63n
console.log(0O7_7n)                          // 63n
console.log(0o77_77n)                        // 4095n

// Hex BigInt with separator (regression — already worked).
console.log(0xa_bn)                          // 171n
console.log(0xff_ffn)                        // 65535n

// Binary BigInt without separator.
console.log(0b101n)                          // 5n
console.log(0b1111_1111_1111_1111n)          // 65535n
