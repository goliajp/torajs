// Adapted from test262: language/expressions/subtraction/S11.6.2_A2.1_T1.js
function check(): number {
  if (1 - 1 !== 0) { throw "#1"; }
  let x: number = 1;
  if (x - 1 !== 0) { throw "#2"; }
  let y: number = 1;
  if (1 - y !== 0) { throw "#3"; }
  if (x - y !== 0) { throw "#4"; }
  return 0;
}
console.log(check());
