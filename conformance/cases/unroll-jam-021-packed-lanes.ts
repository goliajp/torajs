// Escape-time loops whose lanes run two to a vector register: point
// counts that leave a partial group, groups where every lane escapes
// at the first step or none ever does, a count stepped by more than
// one, exits reading the whole point, and NaN, infinite and negative
// zero inputs.
function escapeTime(cr: number, ci: number, max: number, lim: number): number {
  let zr = 0;
  let zi = 0;
  let n = 0;
  while (n < max) {
    if (zr * zr + zi * zi > lim) return n;
    const t = zr * zr - zi * zi + cr;
    zi = 2 * zr * zi + ci;
    zr = t;
    n = n + 1;
  }
  return max;
}

function row(cr: number, side: number, max: number, lim: number): string {
  const out: number[] = [];
  for (let j = 0; j < side; j++) {
    out.push(escapeTime(cr, j / (side + 1) - 0.5, max, lim));
  }
  return out.join(",");
}

// 1..9 points: every remainder of a four-lane group
for (let side = 1; side <= 9; side++) {
  console.log(side, row(-0.75, side, 40, 4));
}
// every lane escapes at its first step: the point is outside the limit at once
console.log(row(3, 7, 40, 4), row(0.1, 7, 40, -1));
// no lane ever escapes: zero and one steps, a NaN limit, an infinite one
console.log(row(0.1, 7, 0, 4), row(0.1, 7, 1, 4), row(0.1, 7, 9, NaN), row(0.1, 7, 9, Infinity));
// NaN, infinite and negative zero points
console.log(row(NaN, 5, 9, 4), row(Infinity, 5, 9, 4), row(-Infinity, 5, 9, 4), row(-0, 5, 9, 4));
console.log(escapeTime(-0, -0, 9, 4), escapeTime(0.3, -0, 9, -0), escapeTime(1e308, 1e308, 9, 1e308));

// the count steps by two, the limit test is `>=`, and the step limit is `<=`
function byTwo(cr: number, ci: number, max: number): number {
  let zr = 0;
  let zi = 0;
  let n = 0;
  while (n <= max) {
    if (zr * zr + zi * zi >= 4) return n;
    const t = zr * zr - zi * zi + cr;
    zi = 2 * zr * zi + ci;
    zr = t;
    n = n + 2;
  }
  return -1;
}

function twos(cr: number, side: number, max: number): string {
  const out: number[] = [];
  for (let j = 0; j < side; j++) {
    out.push(byTwo(cr, j / 4 - 1, max));
  }
  return out.join(",");
}
console.log(twos(-0.75, 11, 30), twos(0.3, 11, 30), twos(0.3, 11, 0), twos(2, 6, 30));

// both exits read the point: every cell is snapshotted
function point(cr: number, ci: number, max: number): number {
  let zr = 0;
  let zi = 0;
  let n = 0;
  while (n < max) {
    if (zr * zr + zi * zi > 4) return zr * 1000 + zi * 10 + n;
    const t = zr * zr - zi * zi + cr;
    zi = 2 * zr * zi + ci;
    zr = t;
    n = n + 1;
  }
  return zr - zi;
}

function points(cr: number, side: number, max: number): number {
  let total = 0;
  for (let j = 0; j < side; j++) {
    total += point(cr, j / 8 - 1, max);
  }
  return total;
}
console.log(points(-0.75, 17, 30), points(0.25, 9, 12), points(-2, 5, 2), points(0.3, 3, 0));

// an integer recurrence under an equality exit: a lane leaves when its
// value lands on the target or its steps run out
function lands(seed: number, target: number, steps: number): number {
  let x = seed;
  let k = 0;
  while (k < steps) {
    if (x === target) return k;
    x = x + x - 5;
    k = k + 1;
  }
  return -steps;
}
function landings(side: number, target: number, steps: number): string {
  const out: number[] = [];
  for (let i = 0; i < side; i++) {
    out.push(lands(i, target, steps));
  }
  return out.join(",");
}
console.log(landings(40, 37, 20), landings(9, 5, 5), landings(6, 0, 0));
