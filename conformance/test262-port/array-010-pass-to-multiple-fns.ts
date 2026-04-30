// Adapted from test262: same array passed to multiple analyses.
function len(xs: number[]): number { return xs.length; }
function sum(xs: number[]): number {
  let s: number = 0;
  for (let i: number = 0; i < xs.length; i = i + 1) { s = s + xs[i]; }
  return s;
}
function first(xs: number[]): number { return xs[0]; }

function check(): number {
  let v: number[] = [10, 20, 30, 40];
  if (len(v) !== 4) { throw "#1"; }
  if (sum(v) !== 100) { throw "#2"; }
  if (first(v) !== 10) { throw "#3"; }
  if (sum(v) + len(v) !== 104) { throw "#4: combined"; }
  return 0;
}
console.log(check());
