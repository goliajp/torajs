// Integration: stats over a number array via a single multi-output
// function. tr's affine model treats non-Copy fn args as transfers
// (rather than TS-shape borrows), so the same array binding can't be
// passed to several functions in sequence — single-pass code instead.
type Stats = { count: number, sum: number, max: number };

function stats(xs: number[]): Stats {
  let s: number = 0;
  let m: number = xs[0];
  for (let i: number = 0; i < xs.length; i = i + 1) {
    s = s + xs[i];
    if (xs[i] > m) { m = xs[i]; }
  }
  return { count: xs.length, sum: s, max: m };
}

function check(): number {
  let v: number[] = [3, 1, 4, 1, 5, 9, 2, 6];
  let r = stats(v);
  if (r.count !== 8) { throw "#1"; }
  if (r.sum !== 31) { throw "#2"; }
  if (r.max !== 9) { throw "#3"; }
  return 0;
}
console.log(check());
