// Adapted from test262: combination of Array.prototype.{map,filter,reduce}
// — exercises the chain composition path that test262 splits across files.
function isEven(n: number): boolean { return n % 2 === 0; }
function double(n: number): number { return n * 2; }
function add(a: number, b: number): number { return a + b; }

function check(): number {
  let xs: number[] = [];
  xs.push(1); xs.push(2); xs.push(3); xs.push(4); xs.push(5); xs.push(6);
  // even numbers, doubled, summed = 2*(2+4+6) = 24
  if (xs.filter(isEven).map(double).reduce(add, 0) !== 24) { throw "#1"; }
  return 0;
}
console.log(check());
