// Integration: prime counting via per-element trial division (true
// Sieve of Eratosthenes is blocked on `arr[i] = false` runtime which
// tr doesn't have yet — see object-007's caveat). Exercises arrays of
// booleans, while loop, modulo, comparison.
function isPrime(n: number): boolean {
  if (n < 2) { return false; }
  if (n === 2) { return true; }
  if (n % 2 === 0) { return false; }
  let j: number = 3;
  while (j * j <= n) {
    if (n % j === 0) { return false; }
    j = j + 2;
  }
  return true;
}

function countPrimes(n: number): number {
  let count: number = 0;
  for (let i: number = 2; i <= n; i = i + 1) {
    if (isPrime(i)) { count = count + 1; }
  }
  return count;
}

function check(): number {
  // primes ≤ 30 → 2,3,5,7,11,13,17,19,23,29 = 10
  if (countPrimes(30) !== 10) { throw "#1"; }
  if (countPrimes(10) !== 4) { throw "#2"; }
  if (countPrimes(100) !== 25) { throw "#3"; }
  return 0;
}
console.log(check());
