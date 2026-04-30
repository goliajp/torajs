// Adapted from test262: built-ins/Array/prototype/push + indexed access.
function check(): number {
  let xs: number[] = [];
  xs.push(10); xs.push(20); xs.push(30);
  if (xs[0] !== 10) { throw "#1"; }
  if (xs[1] !== 20) { throw "#2"; }
  if (xs[2] !== 30) { throw "#3"; }
  if (xs.length !== 3) { throw "#4"; }
  return 0;
}
console.log(check());
