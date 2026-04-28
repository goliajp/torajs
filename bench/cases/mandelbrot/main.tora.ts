function mandel(cr: f64, ci: f64, max_iter: number): number {
  let zr: f64 = 0;
  let zi: f64 = 0;
  let n: number = 0;
  let new_zr: f64 = 0;
  while (n < max_iter) {
    if (zr * zr + zi * zi > 4) return n;
    new_zr = zr * zr - zi * zi + cr;
    zi = 2 * zr * zi + ci;
    zr = new_zr;
    n = n + 1;
  }
  return max_iter;
}

let total: number = 0;
let i: number = 0;
let j: number = 0;
let cr: f64 = 0;
let ci: f64 = 0;
while (i < 200) {
  j = 0;
  while (j < 200) {
    cr = i / 100 - 1.5;
    ci = j / 100 - 1.0;
    total = total + mandel(cr, ci, 1000);
    j = j + 1;
  }
  i = i + 1;
}
console.log(total);
