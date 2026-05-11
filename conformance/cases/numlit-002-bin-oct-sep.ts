// V3-18 m1.h.55 — extended numeric literal forms per JS spec
// §12.8.3:
//   0b... binary
//   0o... octal
//   1_000 numeric separator
//
// Pre-fix tora's lexer only accepted decimal + 0x... hex.

console.log(0b101)            // 5
console.log(0b1111)           // 15
console.log(0B11111111)       // 255 (uppercase B)

console.log(0o17)             // 15
console.log(0o755)            // 493
console.log(0O10)             // 8 (uppercase O)

console.log(1_000_000)        // 1000000
console.log(0xff_ff)          // 65535 (separator in hex too)
console.log(1_2_3_4)          // 1234
console.log(3.14_15)          // 3.1415 (separator in fraction)

// Hex still works.
console.log(0xff)             // 255
console.log(0xABCDEF)         // 11259375

// Decimal still works.
console.log(123)
console.log(1.5e2)            // 150
