// Adapted from test262: built-ins/Array/prototype/reduce.
// MVP: with initial value (no-initial-value overload deferred).
function check(): number {
  let xs: number[] = [];
  xs.push(1); xs.push(2); xs.push(3); xs.push(4); xs.push(5);
  let sum: number = xs.reduce((a: number, x: number): number => a + x, 0);
  if (sum !== 15) { throw "#1"; }
  let prod: number = xs.reduce((a: number, x: number): number => a * x, 1);
  if (prod !== 120) { throw "#2"; }
  return 0;
}
console.log(check());
