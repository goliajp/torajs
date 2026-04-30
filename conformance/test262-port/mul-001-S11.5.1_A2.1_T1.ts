// Adapted from test262: language/expressions/multiplication/S11.5.1_A2.1_T1.js
function check(): number {
  if (1 * 1 !== 1) { throw "#1"; }
  let x: number = 1;
  if (x * 1 !== 1) { throw "#2"; }
  let y: number = 1;
  if (1 * y !== 1) { throw "#3"; }
  if (x * y !== 1) { throw "#4"; }
  return 0;
}
console.log(check());
