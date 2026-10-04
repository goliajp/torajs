// Arrays filled from `0 - i` read back with the range the negation
// gives them; sums over them stay exact while they can, round like f64
// once they pass -2^53, and the zero `0 - 0` writes is still +0.
function num(s: string): number {
  return parseInt(s, 10);
}

const n = num("100000");
let xs: number[] = [];
let i = 0;
while (i < n) {
  xs.push(0 - i);
  i = i + 1;
}
let sum = 0;
let j = 0;
while (j < xs.length) {
  sum = sum + xs[j];
  j = j + 1;
}
console.log(sum);

const big = num("4503599627370490");
let ys: number[] = [];
let k = 0;
while (k < 9) {
  ys.push(0 - (big + k));
  k = k + 1;
}
let t = 0;
let m = 0;
while (m < ys.length) {
  t = t + ys[m];
  m = m + 1;
}
console.log(t);
console.log(ys[8]);

let zs: number[] = [];
let q = 0;
while (q < 2) {
  zs.push(0 - q);
  q = q + 1;
}
console.log(1 / zs[0], Object.is(zs[0], -0));
