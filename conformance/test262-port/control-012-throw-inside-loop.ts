// Adapted from test262: throw inside a for-loop, caught outside.
function find_first_negative(xs: number[]): number {
  for (let i: number = 0; i < xs.length; i = i + 1) {
    if (xs[i] < 0) { throw xs[i]; }
  }
  return 0;
}

function check(): number {
  let arr: number[] = [];
  arr.push(1); arr.push(2); arr.push(-3); arr.push(4);
  let caught: number = 0;
  try {
    find_first_negative(arr);
  } catch (e: number) {
    caught = e;
  }
  if (caught !== -3) { throw "#1"; }

  // Empty array → no throw, returns 0.
  let empty: number[] = [];
  if (find_first_negative(empty) !== 0) { throw "#2"; }
  return 0;
}
console.log(check());
