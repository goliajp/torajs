// P0.10 (lexer extension) — trailing-dot DecimalLiteral followed
// by NON-member-access characters per ES spec §12.9.3
// DecimalLiteral. The earlier trailing-dot+exponent fix only ate
// the dot when followed by `e` / `E`. The general spec-shape
// `8. !== 8` / `9. + 1` / `(7.).toString()` was still bailing
// because peek of byte after the dot is space / operator / `)`,
// not a digit, not `e`/`E`, not another `.`. Test262's
// language/literals/numeric/S7.8.3_A3.1_T*.js (~6 cases) uses
// these forms pervasively.
//
// Implementation: extend the lexer's number-literal post-int loop
// with one more branch — match peek == `.` and peek+1 is anything
// that disqualifies member-access continuation (not alphanumeric,
// not `_`, not `$`, not another `.`). Consume the dot.

console.log(8. !== 8)                        // false
console.log(9.)                              // 9
console.log(7. + 1)                          // 8
console.log(5. * 2)                          // 10
console.log(3. - 1)                          // 2
console.log((4.))                            // 4
console.log(2. === 2)                        // true

// Regression: trailing-dot before `e` / `E` (already covered)
console.log(1.e3)                            // 1000

// Regression: standard fractional (already covered)
console.log(0.5)                             // 0.5

// Regression: `0..toString()` member access (already covered)
console.log((123).toString())                // 123
