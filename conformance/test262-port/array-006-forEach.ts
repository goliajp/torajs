// Adapted from test262: built-ins/Array/prototype/forEach/*.js
// forEach visits each element in order with the callback.
function check(): number {
  let xs: number[] = [];
  xs.push(1); xs.push(2); xs.push(3); xs.push(4); xs.push(5);
  xs.forEach((x: number): void => { console.log(x); });
  return 0;
}
console.log(check());
