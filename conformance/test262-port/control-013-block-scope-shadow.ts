// Adapted from test262: language/statements/let/* — inner block let
// shadows the outer binding without leaking back.
function check(): number {
  let x: number = 10;
  if (x === 10) {
    let x: number = 99;
    if (x !== 99) { throw "#1: inner"; }
  }
  if (x !== 10) { throw "#2: outer restored"; }

  for (let i: number = 0; i < 1; i = i + 1) {
    let x: number = -1;
    if (x !== -1) { throw "#3: for-body"; }
  }
  if (x !== 10) { throw "#4: outer after for"; }
  return 0;
}
console.log(check());
