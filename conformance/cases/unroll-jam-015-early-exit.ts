// Inner loops left early (an inlined `return`, a `break`) under a
// counted outer loop: lanes of a group finish at different steps, and
// each lane's result and the order of the outer loop's effects are the
// original's.
function escapeTime(cr: number, ci: number, maxIter: number): number {
  let zr = 0;
  let zi = 0;
  let n = 0;
  while (n < maxIter) {
    if (zr * zr + zi * zi > 4) return n;
    const t = zr * zr - zi * zi + cr;
    zi = 2 * zr * zi + ci;
    zr = t;
    n = n + 1;
  }
  return maxIter;
}
for (let side = 1; side <= 13; side += 2) {
  let total = 0;
  const rows: string[] = [];
  for (let i = 0; i < side; i++) {
    let row = "";
    for (let j = 0; j < side; j++) {
      const n = escapeTime(i / (side / 2) - 1.5, j / (side / 2) - 1.0, 60);
      total += n * (j + 1);
      row += n + ",";
    }
    rows.push(row);
  }
  console.log(side, total, rows.join("|"));
}

// the step bumps its counter ahead of the exit test
function steps(n: number, lim: number): number {
  let total = 0;
  for (let i = 1; i <= n; i++) {
    let x = i * 0.75;
    let k = 0;
    while (k < 40) {
      k++;
      if (x > lim) break;
      x = x * x * 0.5 + 1;
    }
    total = total * 3 + k;
    if (total > 1e12) total = total % 1000003;
  }
  return total;
}
for (const lim of [0, 1, 2.5, 10, 1e6, 1e300]) console.log(lim, steps(23, lim));

// the first lane leaves at once and its iteration returns; the lanes
// after it never would, and must not be waited for
function firstHit(n: number, lim: number): number {
  for (let i = 1; i <= n; i++) {
    let x = 2 / i;
    let k = 0;
    while (x === x) {
      if (x > lim) break;
      x = x * x;
      k++;
    }
    if (x > lim) return i * 1000 + k;
  }
  return -1;
}
console.log(firstHit(8, 1e100), firstHit(1, 3), firstHit(3, 1e10));

// each lane's exit is seen in iteration order
const seen: number[] = [];
for (let i = 0; i < 11; i++) {
  let x = (i % 4) + 0.5;
  let k = 0;
  while (k < 30) {
    if (x > 50) break;
    x = x * 1.5 + 0.25;
    k++;
  }
  seen.push(k * 100 + i);
}
console.log(seen.join(" "));
