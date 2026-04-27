function gcd(a: number, b: number): number {
  while (b !== 0) {
    const t = b;
    b = a % b;
    a = t;
  }
  return a;
}

let total = 0;
const target = 1234567;
for (let i = 1; i <= 1000000; i++) {
  total += gcd(i, target);
}
console.log(total);
