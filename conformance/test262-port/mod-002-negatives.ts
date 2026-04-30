// Adapted from test262: language/expressions/modulus/S11.5.3_A2.1_T2.js
// Spec for `%` on negatives: result has the sign of the dividend (left
// operand). E.g. -7 % 3 === -1, 7 % -3 === 1.
function check(): number {
  if (-7 % 3 !== -1) { throw "#1"; }
  if (7 % -3 !== 1) { throw "#2"; }
  if (-7 % -3 !== -1) { throw "#3"; }
  if (0 % 5 !== 0) { throw "#4"; }
  if (10 % 10 !== 0) { throw "#5"; }
  return 0;
}
console.log(check());
