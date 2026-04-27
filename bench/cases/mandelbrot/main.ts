function mandel(cr: number, ci: number, max_iter: number): number {
  let zr = 0;
  let zi = 0;
  let n = 0;
  while (n < max_iter) {
    if (zr * zr + zi * zi > 4) return n;
    const new_zr = zr * zr - zi * zi + cr;
    zi = 2 * zr * zi + ci;
    zr = new_zr;
    n = n + 1;
  }
  return max_iter;
}

let total = 0;
for (let i = 0; i < 200; i++) {
  for (let j = 0; j < 200; j++) {
    const cr = i / 100 - 1.5;
    const ci = j / 100 - 1.0;
    total += mandel(cr, ci, 1000);
  }
}
console.log(total);
