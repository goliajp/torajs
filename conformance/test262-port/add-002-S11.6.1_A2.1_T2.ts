// Adapted from test262: language/expressions/addition/S11.6.1_A2.1_T2.js
// Same operator under variable propagation — verifies the assignment +
// readback path doesn't lose precision and the binop sees the latest store.
function check(): number {
  let x: number = 1;
  let y: number = 1;
  x = x + y;
  if (x !== 2) { throw "#1: x=x+y"; }
  y = y + x;
  if (y !== 3) { throw "#2: y=y+x"; }
  let z: number = x + y;
  if (z !== 5) { throw "#3: z"; }
  return 0;
}
console.log(check());
