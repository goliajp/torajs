// branch_fold bounds a reduction over an array by the array's length
// cell (the count a pre-reserved push loop wrote back) times the
// element point, so float_demote's `sum > 2^53-1` guard on
// `for (i = 0; i < xs.length; i++) sum += xs[i]` folds away. The bound
// must stay honest: elements large enough that the product passes
// 2^53 keep the guard and round like bun, and a push inside the loop
// moves the length past what the writeback said, so nothing folds.
function sum(xs: number[]): number {
  let s = 0;
  for (let i = 0; i < xs.length; i = i + 1) {
    s = s + xs[i];
  }
  return s;
}
// well inside 2^53: the guard folds, the i64 sum is exact
const small: number[] = [];
for (let i = 0; i < 100000; i = i + 1) {
  small.push(i * 1000);
}
console.log(sum(small));
// length × element bound passes 2^53: the guard stays and the f64
// sum rounds past 2^53 the way bun's does
const big: number[] = [];
for (let i = 0; i < 4; i = i + 1) {
  big.push(2 ** 52 + i);
}
console.log(sum(big));
// the length moves inside the loop: the header's length read has no
// single writer to resolve to
const grow: number[] = [];
for (let i = 0; i < 3; i = i + 1) {
  grow.push(2 ** 52);
}
let g = 0;
for (let i = 0; i < grow.length; i = i + 1) {
  if (i < 3) grow.push(2 ** 52);
  g = g + grow[i];
}
console.log(g, grow.length);
