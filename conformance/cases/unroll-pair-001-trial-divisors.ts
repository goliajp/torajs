// Divisibility loops whose quotients come two at a time from one
// two-lane float divide: trip counts of zero, one, two and odd length,
// the divisor found in the first or the second lane, a second-lane
// divisor past the loop bound that must not count, the counter read
// after the loop, a step of two, dividends at the top of the exact
// window, and the shapes that keep one quotient per step.

// smallest divisor of n in [2, sqrt n], 0 for a prime: exits in lane 0
// (n = 4, 6, 25) and in lane 1 (n = 9, 15, 49), runs no step (n < 4),
// one step (n = 5, 7), an odd number of steps (n = 29: i = 2..5)
function smallestFactor(n: number): number {
  let i = 2;
  while (i * i <= n) {
    if (n % i === 0) return i;
    i = i + 1;
  }
  return 0;
}

// the counter after the loop is the value the last step left
function stepsTaken(n: number): number {
  let i = 2;
  while (i * i <= n) {
    if (n % i === 0) break;
    i = i + 1;
  }
  return i;
}

// a bound the second lane may pass: for x = 61, d runs 2..60 and the
// divisor 61 sits in the lane past the bound, so none is found
function smallestIn(x: number, from: number, to: number): number {
  for (let d = from; d <= to; d++) {
    if (x % d === 0) return d;
  }
  return 0;
}

// odd divisors only: a step of two
function smallestOdd(n: number): number {
  let d = 3;
  while (d * d <= n) {
    if (n % d === 0) return d;
    d += 2;
  }
  return 0;
}

// dividends at the top of the exact window
function topFactors(k: number): string {
  let out = "";
  for (let x = 9007199254740991 - k; x <= 9007199254740991; x++) {
    out += smallestIn(x, 2, 100) + " ";
  }
  return out;
}

// the remainder read as a value keeps one quotient per step
function remainderSum(n: number): number {
  let s = 0;
  let i = 1;
  while (i * i <= n) {
    s += n % i;
    i = i + 1;
  }
  return s;
}

// a quotient read back as a float keeps one per step
function quotientSum(n: number): number {
  let s = 0;
  let i = 1;
  while (i * i <= n) {
    s += Math.trunc(n / i);
    i = i + 1;
  }
  return s;
}

const lim = parseInt(process.argv[2] ?? "200", 10);
let line = "";
for (let n = 0; n < 60; n++) line += smallestFactor(n) + ",";
console.log(line);
line = "";
for (let n = 0; n < 60; n++) line += stepsTaken(n) + ",";
console.log(line);
let primes = 0;
for (let n = 2; n < lim; n++) if (smallestFactor(n) === 0) primes++;
console.log(primes, smallestFactor(lim * lim + 1), smallestFactor(1000003 * 1000003));
console.log(smallestIn(61, 2, 60), smallestIn(61, 2, 61), smallestIn(122, 2, 60), smallestIn(183, 3, 60));
console.log(smallestIn(0, 2, 5), smallestIn(1, 2, 5), smallestIn(7, 8, 7), smallestIn(49, 2, 7));
line = "";
for (let n = 1; n < 80; n += 2) line += smallestOdd(n) + ",";
console.log(line);
console.log(topFactors(parseInt(process.argv[3] ?? "6", 10)));
console.log(remainderSum(lim), remainderSum(lim * 97), quotientSum(lim), quotientSum(lim * 97));
