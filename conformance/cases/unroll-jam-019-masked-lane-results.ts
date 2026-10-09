// Masked groups whose lanes run very different step counts (some none
// at all), outer counts that are not a multiple of the lane count, and
// every lane's final state escaping through an array or a return.
function counts(n: number, scale: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) {
    // with a large scale, lanes with i % 7 >= 2 start past the exit and
    // run no step, while i % 7 == 0 runs all 500
    let x = 3.9 + (i % 7) * scale;
    let k = 0;
    while (k < 500) {
      if (x > 50) break;
      x = x * x * 0.25 + 0.01;
      k++;
    }
    out.push(k);
  }
  return out;
}
for (const n of [0, 1, 2, 3, 5, 7, 13, 30]) console.log(n, counts(n, 30).join(","));
console.log(counts(17, 0.3).join(","));
console.log(counts(17, 1e-3).join(","));

function finals(n: number): number[] {
  const xs: number[] = [];
  for (let i = 1; i <= n; i++) {
    let x = i * 0.2;
    let y = 1;
    let k = 0;
    while (k < 64) {
      if (x * y > 1000) break;
      const t = x * 1.1 + y * 0.01;
      y = y * 1.05 + x * 0.001;
      x = t;
      k++;
    }
    xs.push(x, y, k);
  }
  return xs;
}
console.log(finals(1).join(" "));
console.log(finals(6).join(" "));
console.log(finals(11).join(" "));

function firstOver(n: number, lim: number): number {
  for (let i = 1; i <= n; i++) {
    let x = i * 0.01;
    let k = 0;
    while (k < 1000) {
      if (x > lim) break;
      x = x * 1.5 + 0.001;
      k++;
    }
    if (k > 20) return i * 10000 + k;
  }
  return -1;
}
console.log(firstOver(9, 1), firstOver(9, 1e9), firstOver(3, 1e300), firstOver(0, 1));
