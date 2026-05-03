// Prime sieve — Sieve of Eratosthenes up to N. Prints the count of
// primes ≤ N and the first / last 10 primes for verification.
//
// Exercises: large `boolean[]` arrays, nested for loops, integer
// arithmetic, dynamic array push.

function sieveOfEratosthenes(n: number): number[] {
  // is_composite[i] = true iff `i` has been crossed out.
  const isComposite: boolean[] = [];
  for (let i = 0; i <= n; i++) {
    isComposite.push(false);
  }
  for (let i = 2; i * i <= n; i++) {
    if (!isComposite[i]) {
      let j = i * i;
      while (j <= n) {
        isComposite[j] = true;
        j = j + i;
      }
    }
  }
  const primes: number[] = [];
  for (let i = 2; i <= n; i++) {
    if (!isComposite[i]) {
      primes.push(i);
    }
  }
  return primes;
}

function joinDecimal(xs: number[]): string {
  let out = "";
  for (let i = 0; i < xs.length; i++) {
    if (i > 0) out = out + ", ";
    out = out + xs[i].toString();
  }
  return out;
}

const N = 1000;
const primes = sieveOfEratosthenes(N);

console.log("primes <= " + N.toString() + ": " + primes.length.toString());

const firstTen: number[] = [];
for (let i = 0; i < 10 && i < primes.length; i++) {
  firstTen.push(primes[i]);
}
console.log("first 10: " + joinDecimal(firstTen));

const lastTen: number[] = [];
const start = primes.length - 10;
for (let i = start; i < primes.length; i++) {
  lastTen.push(primes[i]);
}
console.log("last 10:  " + joinDecimal(lastTen));
