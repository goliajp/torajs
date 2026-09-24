// A hole in a number[] of fractional elements — left by a write past the
// end or by growing length — reads as undefined, through the typed lane
// and through an any view alike, and arithmetic on it is NaN.
let y = [1.5, 2.5];
y[4] = 3.5;
console.log(y[3], y[3] === undefined, typeof y[3], y[3] + 1, 3 in y, y.length);
console.log(y[2] ?? 7);
let g = [0.5, 2.5];
g.length = 4;
console.log(g[3], 3 in g, g.length);
const a: any = y;
console.log(a[3], a[4]);
console.log(y);
console.log(y.join("|"), JSON.stringify(y));
