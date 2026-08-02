// §13.16 comma-operator expression statement — plain assigns, index
// assigns, for-init comma, and dstr-assign segments.
let a = 0;
let b = 0;
a = 1, b = 2;
console.log(a, b);

const xs: any[] = [0, 0, 0];
xs[0] = 1, xs[1] = 2, xs[2] = 3;
console.log(xs[0], xs[1], xs[2]);

let i = 0;
let j = 0;
let n = 0;
for (i = 0, j = 9; i < 2; i++, j--) {
  n = n + j;
}
console.log(i, j, n);

let x = 0;
let y = 0;
[x] = [5], [y] = [6];
console.log(x, y);
