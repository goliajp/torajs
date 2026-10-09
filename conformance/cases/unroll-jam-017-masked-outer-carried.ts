// An inner loop left early whose start depends on the previous outer
// iteration's result: a later lane cannot start before an earlier one
// finishes, so the outer loop runs as written.
function chained(n: number, lim: number): number {
  let prev = 0.25;
  let total = 0;
  for (let i = 1; i <= n; i++) {
    let x = prev + i * 0.125;
    let k = 0;
    while (k < 40) {
      if (x > lim) break;
      x = x * x * 0.5 + 1;
      k++;
    }
    prev = x * 0.001;
    total = total * 3 + k;
    if (total > 1e12) total = total % 1000003;
  }
  return total + prev;
}
for (const lim of [0, 1.5, 10, 1e6, 1e300]) console.log(lim, chained(13, lim));

// the counter the inner loop starts from is carried across outer steps
function carriedCounter(n: number): number {
  let k = 0;
  let total = 0;
  for (let i = 1; i <= n; i++) {
    let x = i * 0.5;
    while (k < 200) {
      if (x > 40) break;
      x = x * x * 0.5 + 1;
      k++;
    }
    total = total * 3 + k;
    if (total > 1e12) total = total % 1000003;
  }
  return total;
}
console.log(carriedCounter(17));
