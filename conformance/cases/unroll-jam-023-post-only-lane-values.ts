// An escape-time loop beside seven values each point computes ahead of
// it and reads only after it: they stay one scalar per point instead of
// taking vector registers, and the point's own state still fits.
function heavy(cr: number, ci: number, max: number): number {
  const a = ci * 2;
  const b = ci * 3;
  const c = ci * 5;
  const d = ci * 7;
  const e = ci * 11;
  const f = ci * 13;
  const g = ci * 17;
  let zr = 0;
  let zi = 0;
  let n = 0;
  while (n < max) {
    if (zr * zr + zi * zi > 4) return zr * 1000 + zi * 10 + n + a + b + c + d + e + f + g;
    const t = zr * zr - zi * zi + cr;
    zi = 2 * zr * zi + ci;
    zr = t;
    n = n + 1;
  }
  return zr - zi + a - b + c - d + e - f + g;
}

function total(cr: number, side: number, max: number): number {
  let sum = 0;
  for (let j = 0; j < side; j++) {
    sum += heavy(cr, j / 8 - 1, max);
  }
  return sum;
}

console.log(total(-0.75, 17, 30), total(0.25, 9, 12), total(-2, 5, 2), total(0.3, 3, 0));
console.log(total(0.26, 23, 40), total(-1.2, 31, 25), total(NaN, 6, 5));
