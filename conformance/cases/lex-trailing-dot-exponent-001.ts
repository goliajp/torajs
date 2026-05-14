// P0.10 (lexer extension) — two more numeric-literal shapes
// from ES spec §12.9.3 DecimalLiteral that the v3 lexer was
// dropping:
//
//   (a) Trailing-dot before exponent: `1.e5` / `1.E-3`. Per
//       spec the trailing `.` is part of the DecimalIntegerLiteral
//       and the exponent attaches to that. Pre-fix tora's lexer
//       only consumed the dot when a fractional digit followed,
//       so `1.e5` parsed as `Number(1)` then `.e5` as a member
//       access, bailing at the second character. Test262's
//       language/literals/numeric/S7.8.3_A3.3_T*.js (8 cases)
//       and S7.8.3_A4.1_T*.js (8 cases) both hit this.
//
//   (b) Numeric separator `_` inside exponent digits per
//       ES2021. Pre-fix the exponent-consume loop only accepted
//       digits. Test262's
//       language/literals/numeric/numeric-separators/numeric-
//       separator-literal-dd-dot-dd-ep-sign-*-dd-nsl-dd.js
//       cases hit this (~4+).

// Trailing-dot exponent.
console.log(1.e1)                            // 10
console.log(1.E1)                            // 10
console.log(1.e-3)                           // 0.001
console.log(2.E2)                            // 200
console.log(1.e+5)                           // 100000

// Exponent with separator.
console.log(1.5e1_0)                         // 15000000000
console.log(2.5e+1_0)                        // 25000000000
console.log(3e-1_2)                          // 3e-12
console.log(1.0e1_0)                         // 10000000000

// Regression: existing forms still work.
console.log(0.5)                             // 0.5
console.log(1.5e3)                           // 1500
console.log(1.5e+3)                          // 1500
console.log(1.5e-3)                          // 0.0015
console.log(1)                               // 1

// Regression: trailing-dot member access (e.g. `0..toString()`)
// still works.
console.log((123).toString())                // 123
