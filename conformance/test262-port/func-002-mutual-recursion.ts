// Adapted from test262: forward-reference between two functions calling
// each other (isEven / isOdd). Verifies hoisting + mutual reference.
function isEven(n: number): boolean {
  if (n === 0) { return true; }
  return isOdd(n - 1);
}
function isOdd(n: number): boolean {
  if (n === 0) { return false; }
  return isEven(n - 1);
}

function check(): number {
  if (isEven(0) !== true) { throw "#1"; }
  if (isOdd(1) !== true) { throw "#2"; }
  if (isEven(10) !== true) { throw "#3"; }
  if (isOdd(7) !== true) { throw "#4"; }
  if (isEven(7) !== false) { throw "#5"; }
  return 0;
}
console.log(check());
