// Escape-time loops whose count is bumped ahead of an exit: a lane that
// leaves there has already stepped its count, so the count it keeps for
// the other exit is the one from before the step. A point that escapes
// exactly at the step reaching the limit takes the escape exit.

// the escape exit returns a constant, the limit exit the count
function bumpFirst(cr: number, ci: number, max: number): number {
  let zr = 0;
  let zi = 0;
  let n = 0;
  while (n < max) {
    const t = zr * zr - zi * zi + cr;
    zi = 2 * zr * zi + ci;
    zr = t;
    n = n + 1;
    if (zr * zr + zi * zi > 4) return -1;
  }
  return n;
}

// the sum is taken ahead of the escape test and stored after the step
function sumFirst(cr: number, ci: number, max: number): number {
  let zr = 0;
  let zi = 0;
  let n = 0;
  while (n < max) {
    const m = n + 1;
    if (zr * zr + zi * zi > 4) return n;
    const t = zr * zr - zi * zi + cr;
    zi = 2 * zr * zi + ci;
    zr = t;
    n = m;
  }
  return -max;
}

function rowBump(cr: number, side: number, max: number): string {
  const out: number[] = [];
  for (let j = 0; j < side; j++) {
    out.push(bumpFirst(cr, j / side - 1, max));
  }
  return out.join(",");
}

function rowSum(cr: number, side: number, max: number): string {
  const out: number[] = [];
  for (let j = 0; j < side; j++) {
    out.push(sumFirst(cr, j / side - 1, max));
  }
  return out.join(",");
}

for (let max = 0; max <= 9; max++) {
  console.log(max, rowBump(-0.75, 13, max), rowBump(0.3, 11, max), rowBump(-1.9, 7, max));
  console.log(max, rowSum(-0.75, 13, max), rowSum(0.3, 11, max), rowSum(-1.9, 7, max));
}
console.log(rowBump(0.26, 17, 30), rowSum(0.26, 17, 30));
