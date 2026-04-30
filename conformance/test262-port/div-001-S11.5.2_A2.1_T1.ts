// Adapted from test262: language/expressions/division/S11.5.2_A2.1_T1.js
// Mechanical rewrite: var → let, `throw new Test262Error` → `throw "..."`.
function check(): number {
  if (4 / 2 !== 2) { throw "#1: 4/2!==2"; }
  let x: number = 6;
  if (x / 2 !== 3) { throw "#2: x/2!==3"; }
  let y: number = 3;
  if (9 / y !== 3) { throw "#3: 9/y!==3"; }
  if (x / y !== 2) { throw "#4: x/y!==2"; }
  return 0;
}
console.log(check());
