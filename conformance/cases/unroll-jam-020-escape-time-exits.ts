// Escape-time inner loops whose two exits (the step limit, the escape
// test) are followed by code reading different cells: only the count,
// the point on one exit, the point on both, and NaN parameters that
// keep the escape test from ever firing or make it fire at once.
function count(cr: number, ci: number, max: number): number {
  let zr = 0;
  let zi = 0;
  let n = 0;
  while (n < max) {
    if (zr * zr + zi * zi > 4) return n;
    const t = zr * zr - zi * zi + cr;
    zi = 2 * zr * zi + ci;
    zr = t;
    n = n + 1;
  }
  return max;
}

function grid(side: number, max: number): number {
  let total = 0;
  for (let i = 0; i < side; i++) {
    for (let j = 0; j < side; j++) {
      total += count(i / 10 - 1.5, j / 10 - 1.0, max);
    }
  }
  return total;
}
console.log(grid(21, 50), grid(7, 1), grid(5, 0), grid(13, 3));

// the escape exit reads the point, the limit exit only `max`
function escapePoint(cr: number, ci: number, max: number): number {
  let zr = 0;
  let zi = 0;
  let n = 0;
  while (n < max) {
    if (zr * zr + zi * zi > 4) return n * 1000 + zr;
    const t = zr * zr - zi * zi + cr;
    zi = 2 * zr * zi + ci;
    zr = t;
    n = n + 1;
  }
  return -max;
}

// the limit exit reads the point, the escape exit only the count
function limitPoint(cr: number, ci: number, max: number): number {
  let zr = 0;
  let zi = 0;
  let n = 0;
  while (n < max) {
    if (zr * zr + zi * zi > 4) return n;
    const t = zr * zr - zi * zi + cr;
    zi = 2 * zr * zi + ci;
    zr = t;
    n = n + 1;
  }
  return zr * 1000 + n;
}

// both exits read the whole point
function bothPoint(cr: number, ci: number, max: number): number {
  let zr = 0;
  let zi = 0;
  let n = 0;
  while (n < max) {
    if (zr * zr + zi * zi > 4) return zr + zi + n;
    const t = zr * zr - zi * zi + cr;
    zi = 2 * zr * zi + ci;
    zr = t;
    n = n + 1;
  }
  return zr - zi;
}

function rows(cr: number, side: number, max: number): string {
  let a = 0;
  let b = 0;
  let c = 0;
  for (let j = 0; j < side; j++) {
    a += escapePoint(cr, j / 8 - 1, max);
  }
  for (let j = 0; j < side; j++) {
    b += limitPoint(cr, j / 8 - 1, max);
  }
  for (let j = 0; j < side; j++) {
    c += bothPoint(cr, j / 8 - 1, max);
  }
  return a + " " + b + " " + c;
}
console.log(rows(-0.75, 17, 30));
console.log(rows(0.25, 17, 12));
console.log(rows(-2, 9, 2));
console.log(rows(NaN, 9, 20));

// NaN parameters: a NaN limit never lets a lane escape, a NaN point
// stays NaN, and a negative limit lets every lane escape at once
function counts(side: number, max: number, cr: number, lim: number): string {
  const out: number[] = [];
  for (let j = 0; j < side; j++) {
    const ci = j / 5 - 1;
    let zr = 0;
    let zi = 0;
    let n = 0;
    let r = max;
    while (n < max) {
      if (zr * zr + zi * zi > lim) {
        r = n;
        break;
      }
      const t = zr * zr - zi * zi + cr;
      zi = 2 * zr * zi + ci;
      zr = t;
      n++;
    }
    out.push(r);
  }
  return out.join(",");
}
console.log(counts(11, 25, 0.3, NaN));
console.log(counts(11, 25, NaN, 4));
console.log(counts(11, 25, -0.1, -1));
console.log(counts(11, 25, 0.26, 4));
