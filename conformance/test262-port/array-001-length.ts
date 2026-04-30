// Adapted from test262: built-ins/Array/length/*.js
// Drops cases setting array.length to truncate (we don't support that yet).
function check(): number {
  let xs: number[] = [];
  if (xs.length !== 0) { throw "#1"; }
  xs.push(1);
  if (xs.length !== 1) { throw "#2"; }
  xs.push(2); xs.push(3);
  if (xs.length !== 3) { throw "#3"; }
  return 0;
}
console.log(check());
