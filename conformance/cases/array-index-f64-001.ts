// f64-valued index expressions on typed arrays — ToInteger coerce
// (read / write / post-incr paths; nested-neg S8 shape included)
let xs: number[] = [10, 20, 30];
let i: number = 6 / 3;
console.log(xs[i]);
console.log(xs[2 / 2]);
let j: number = 2.0;
console.log(xs[-(-j)]);
xs[4 / 2] = 99;
console.log(xs[2]);
xs[2 / 2]++;
console.log(xs[1]);
console.log(xs[i - 1]);
