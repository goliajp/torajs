let xs: number[] = [];
let i: number = 0;
while (i < 10000000) {
  xs.push(i);
  i = i + 1;
}
let sum: number = 0;
let j: number = 0;
while (j < xs.length) {
  sum = sum + xs[j];
  j = j + 1;
}
console.log(sum);
