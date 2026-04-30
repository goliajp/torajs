// Adapted from test262: language/expressions/subtraction/S11.6.2_A2.1_T2.js
function check(): number {
  if (5 - 3 !== 2) { throw "#1"; }
  if (3 - 5 !== -2) { throw "#2"; }
  if (-5 - -3 !== -2) { throw "#3"; }
  if (-5 - 3 !== -8) { throw "#4"; }
  if (0 - 0 !== 0) { throw "#5"; }
  let a: number = 100;
  let b: number = 75;
  if (a - b !== 25) { throw "#6"; }
  return 0;
}
console.log(check());
