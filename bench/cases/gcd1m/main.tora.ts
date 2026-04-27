function gcd(a: number, b: number): number {
  let t: number = 0;
  while (b !== 0) {
    t = b;
    b = a % b;
    a = t;
  }
  return a;
}

let total: number = 0;
let i: number = 1;
let target: number = 1234567;
while (i <= 1000000) {
  total = total + gcd(i, target);
  i = i + 1;
}
console.log(total);
