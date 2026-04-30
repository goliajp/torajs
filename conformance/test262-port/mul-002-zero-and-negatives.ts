// Adapted from test262: language/expressions/multiplication/S11.5.1_A2.1_T2.js
function check(): number {
  if (0 * 100 !== 0) { throw "#1"; }
  if (100 * 0 !== 0) { throw "#2"; }
  if (-3 * 4 !== -12) { throw "#3"; }
  if (-3 * -4 !== 12) { throw "#4"; }
  if (1 * 1 !== 1) { throw "#5"; }
  let a: number = 7;
  let b: number = -8;
  if (a * b !== -56) { throw "#6"; }
  return 0;
}
console.log(check());
