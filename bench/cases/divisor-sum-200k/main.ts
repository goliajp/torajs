function divisorSum(n: number): number {
  let s: number = 0;
  let d: number = 1;
  while (d * d <= n) {
    if (n % d === 0) s = s + d;
    d = d + 1;
  }
  return s;
}

const limit: number = parseInt(process.argv[2], 10);
let total: number = 0;
let n: number = 1;
while (n <= limit) {
  total = total + divisorSum(n);
  n = n + 1;
}
console.log(total);
