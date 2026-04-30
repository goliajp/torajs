// Adapted from test262: built-ins/Array/prototype/map.
// MVP: single-arg callback (we don't pass index/array — TS allows that
// signature, the test262 cases that rely on the second/third arg are dropped).
function check(): number {
  let xs: number[] = [];
  xs.push(1); xs.push(2); xs.push(3);
  let ys: number[] = xs.map((x: number): number => x * 2);
  if (ys[0] !== 2) { throw "#1"; }
  if (ys[1] !== 4) { throw "#2"; }
  if (ys[2] !== 6) { throw "#3"; }
  if (ys.length !== 3) { throw "#4"; }
  return 0;
}
console.log(check());
