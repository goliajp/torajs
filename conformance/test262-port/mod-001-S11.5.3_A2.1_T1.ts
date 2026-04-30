// Adapted from test262: language/expressions/modulus/S11.5.3_A2.1_T1.js
// Mechanical rewrite: var → let, `throw new Test262Error` → `throw "..."`.
function check(): number {
  if (7 % 2 !== 1) { throw "#1: 7%2!==1"; }
  let x: number = 10;
  if (x % 3 !== 1) { throw "#2: x%3!==1"; }
  let y: number = 4;
  if (15 % y !== 3) { throw "#3: 15%y!==3"; }
  if (x % y !== 2) { throw "#4: x%y!==2"; }
  return 0;
}
console.log(check());
