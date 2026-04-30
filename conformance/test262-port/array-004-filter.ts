// Adapted from test262: built-ins/Array/prototype/filter.
function check(): number {
  let xs: number[] = [];
  xs.push(1); xs.push(2); xs.push(3); xs.push(4); xs.push(5);
  let evens: number[] = xs.filter((x: number): boolean => x % 2 === 0);
  if (evens.length !== 2) { throw "#1"; }
  if (evens[0] !== 2) { throw "#2"; }
  if (evens[1] !== 4) { throw "#3"; }
  return 0;
}
console.log(check());
