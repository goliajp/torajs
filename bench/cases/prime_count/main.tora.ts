function is_prime(n: number): boolean {
  if (n < 2) return false;
  let i: number = 2;
  while (i * i <= n) {
    if (n % i === 0) return false;
    i = i + 1;
  }
  return true;
}

let count: number = 0;
let n: number = 0;
while (n < 1000000) {
  if (is_prime(n)) {
    count = count + 1;
  }
  n = n + 1;
}
console.log(count);
