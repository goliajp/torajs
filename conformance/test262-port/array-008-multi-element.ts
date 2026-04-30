// Adapted from test262: built-ins/Array/prototype/push.js +
// length growth. Verifies the array slot expands with each push.
function check(): number {
  let xs: number[] = [];
  if (xs.length !== 0) { throw "#1"; }
  for (let i: number = 0; i < 100; i = i + 1) {
    xs.push(i * i);
  }
  if (xs.length !== 100) { throw "#2"; }
  if (xs[0] !== 0) { throw "#3"; }
  if (xs[10] !== 100) { throw "#4"; }
  if (xs[99] !== 9801) { throw "#5"; }
  return 0;
}
console.log(check());
