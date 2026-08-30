// Same stale-bound defect on the `while` lane.
function f(): number {
  let xs: number[] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
  let i: number = 0;
  while (i < (xs.length >> 1)) { xs.push(i); i++; }
  return xs.length;
}
console.log(f());
