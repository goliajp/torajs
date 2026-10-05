function smallestFactor(n: number): number {
  let d: number = 2;
  while (d * d <= n) {
    if (n % d === 0) return d;
    d = d + 1;
  }
  return n;
}

const limit: number = parseInt(process.argv[2], 10);
let total: number = 0;
let n: number = 2;
while (n < limit) {
  total = total + smallestFactor(n);
  n = n + 1;
}
console.log(total);
