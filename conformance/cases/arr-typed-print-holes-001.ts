// A typed array prints its holes — a write past the end or a longer length
// leaves them — as bun's `empty item` / `N x empty items` runs, nested too.
let y = [1.5, 2.5];
y[4] = 3.5;
console.log(y);
let g = [0.5, 2.5];
g.length = 4;
console.log(g);
let h = [0.5, 2.5];
h[3] = 1.5;
console.log(h);
const a: any = g;
console.log(a);
let k = [1, 2];
k[4] = 5;
console.log(k);
const s = ["a"];
s[3] = "d";
console.log(s);
const big: number[] = [];
big[12] = 1.5;
console.log(big);
const b = [true];
b[2] = false;
console.log(b, [y, k]);
