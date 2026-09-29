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

const side = parseInt(process.argv[2], 10);
const max_iter = parseInt(process.argv[3], 10);
let total = 0;
for (let i = 0; i < side; i++) {
  for (let j = 0; j < side; j++) {
    const cr = i / 100 - 1.5;
    const ci = j / 100 - 1.0;
    total += mandel(cr, ci, max_iter);
  }
}
console.log(total);
