// Adapted from test262: language/expressions/addition/S11.6.1_A2.1_T1.js
// Mechanical rewrite: var → let, `throw new Test262Error(msg)` → `throw msg`,
// CHECK#5 dropped (uses `new Object()`).
function check(): number {
  if (1 + 1 !== 2) { throw "#1: 1+1!==2"; }
  let x: number = 1;
  if (x + 1 !== 2) { throw "#2: x+1!==2"; }
  let y: number = 1;
  if (1 + y !== 2) { throw "#3: 1+y!==2"; }
  if (x + y !== 2) { throw "#4: x+y!==2"; }
  return 0;
}
console.log(check());
