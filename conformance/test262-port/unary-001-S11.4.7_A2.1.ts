// Adapted from test262: language/expressions/unary-minus/S11.4.7_A2.1_T1.js
// Negation is the spec's "ToNumber then negate"; we exercise the integer
// path only (we don't yet have NaN / Infinity / -0 distinctions).
function check(): number {
  if (-5 !== 0 - 5) { throw "#1: -5"; }
  let x: number = 7;
  if (-x !== -7) { throw "#2: -x"; }
  if (-(-x) !== 7) { throw "#3: --x"; }
  if (-(x + 1) !== -8) { throw "#4: -(x+1)"; }
  return 0;
}
console.log(check());
