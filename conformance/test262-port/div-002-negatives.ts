// Adapted from test262: language/expressions/division/S11.5.2_A2.1_T2.js
// Integer-friendly divisions only; non-integer results are skipped because
// printing equality on doubles depends on platform formatting.
function check(): number {
  if (-10 / 2 !== -5) { throw "#1"; }
  if (10 / -2 !== -5) { throw "#2"; }
  if (-10 / -2 !== 5) { throw "#3"; }
  if (0 / 7 !== 0) { throw "#4"; }
  let a: number = 100;
  let b: number = -25;
  if (a / b !== -4) { throw "#5"; }
  return 0;
}
console.log(check());
