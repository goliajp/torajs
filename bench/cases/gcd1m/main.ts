function gcd(a: number, b: number): number {
  while (b !== 0) {
    const t = b;
    b = a % b;
    a = t;
  }
  return a;
}

let total = 0;
const n = parseInt(process.argv[2], 10);
const target = parseInt(process.argv[3], 10);
for (let i = 1; i <= n; i++) {
  total += gcd(i, target);
}
console.log(total);
