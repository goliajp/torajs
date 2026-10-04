// Integer arrays with both signs, summed: exact while the running sum
// stays inside ±2^53, rounded like f64 once it leaves.
function num(s: string): number {
  return parseInt(s, 10);
}

const n = num("24");
let xs: number[] = [];
let i = 0;
while (i < n) {
  if (i % 2 === 0) {
    xs.push(i * 1125899906842624);
  } else {
    xs.push(-i);
  }
  i = i + 1;
}
let sum = 0;
let j = 0;
while (j < xs.length) {
  sum = sum + xs[j];
  j = j + 1;
}
console.log(sum);

// the negative side: past -2^53 from below
let ys: number[] = [];
let k = 0;
while (k < n) {
  if (k % 2 === 0) {
    ys.push(k);
  } else {
    ys.push(-(k * 1125899906842624));
  }
  k = k + 1;
}
let t = 0;
let m = 0;
while (m < ys.length) {
  t = t + ys[m];
  m = m + 1;
}
console.log(t);

// alternating signs that cancel: exact all the way
const big = num("1000000");
let zs: number[] = [];
let q = 0;
while (q < big) {
  if (q % 2 === 0) {
    zs.push(q);
  } else {
    zs.push(-q);
  }
  q = q + 1;
}
let u = 0;
let r = 0;
while (r < zs.length) {
  u = u + zs[r];
  r = r + 1;
}
console.log(u, 1 / zs[0]);
