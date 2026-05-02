let xs: number[] = [];
let n: number = 1000000;
for (let i: number = 0; i < n; i = i + 1) { xs.push(i); }
let total: number = 0;
while (xs.length > 0) {
  total = total + xs.pop();
}
console.log(total);
