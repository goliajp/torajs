// Integer remainders in loops whose quotient comes from a float
// division, at the edges of the window it is exact in, and the shapes
// that must keep the integer division.

function countPrimes(limit: number): number {
  let count = 0;
  for (let n = 0; n < limit; n++) {
    let prime = n >= 2;
    for (let i = 2; i * i <= n; i++) {
      if (n % i === 0) {
        prime = false;
        break;
      }
    }
    if (prime) count++;
  }
  return count;
}

// smallest divisor of x in [from, k], 0 when none: dividend 0,
// divisor 1 (from = 1), divisors above the dividend (x = 1, from = 2)
function smallest(n: number, from: number, k: number): number {
  let s = 0;
  for (let x = 0; x < n; x++) {
    let found = 0;
    for (let d = from; d <= k; d++) {
      if (x % d === 0) {
        found = d;
        break;
      }
    }
    s = ((s * 7) % 1000003) + found;
  }
  return s;
}

// a divisibility test inside the loop body keeps the integer division
function divCount(n: number, k: number): number {
  let c = 0;
  for (let x = 0; x < n; x++) {
    for (let d = 1; d <= k; d++) {
      if (x % d === 0) c++;
      if (x % d !== 0) c += 2;
    }
  }
  return c;
}

// a remainder read as a value keeps the integer division
function remSum(n: number, k: number): number {
  let s = 0;
  for (let x = 0; x < n; x++) {
    for (let d = 1; d <= k; d++) {
      s += x % d;
    }
  }
  return s;
}

// dividends just below 2^53, smallest divisor in [2, k]
function topSmallest(k: number): string {
  let out = "";
  for (let x = 9007199254740991; x > 9007199254740991 - 5; x--) {
    let found = 0;
    for (let d = 2; d <= k; d++) {
      if (x % d === 0) {
        found = d;
        break;
      }
    }
    out += found + " ";
  }
  return out;
}

// divisors from 2^53 - 4 up past 2^53, against a dividend of
// 2^53 - 1; the start comes from argv (0 when absent)
function bigDivisors(k: number): string {
  let out = "";
  const x = 9007199254740991;
  for (let y = 9007199254740988 + k; y <= 9007199254740996 + k; y += 2) {
    out += (x % y === 0 ? "d" : "n") + (y > 9007199254740992 ? "+" : "-");
  }
  return out;
}

// a divisibility test whose answer feeds the next dividend, inside
// the loop body: keeps the integer division
function stepCount(n: number, m: number): number {
  let x = 0;
  for (let i = 0; i < n; i++) {
    for (let y = 2; y < m; y++) {
      x = (x + (x % y === 0 ? 1 : 0) + 1) % 1000003;
    }
  }
  return x;
}

// the remainder feeds its own divisor: keeps the integer division
function gcd(a: number, b: number): number {
  while (b !== 0) {
    const t = b;
    b = a % b;
    a = t;
  }
  return a;
}

function gcdSum(n: number, m: number): number {
  let s = 0;
  for (let i = 1; i <= n; i++) s += gcd(i, m);
  return s;
}

// outside the window: negative, fractional, zero, NaN and infinite
// operands go through the float remainder
function rems(xs: number[], ys: number[]): string {
  let out = "";
  for (let i = 0; i < xs.length; i++) {
    for (let j = 0; j < ys.length; j++) {
      out += String(xs[i] % ys[j]) + " ";
    }
  }
  return out;
}

const lim = parseInt(process.argv[2] ?? "2000", 10);
console.log(countPrimes(lim));
console.log(remSum(200, 60));
console.log(smallest(200, 1, 60));
console.log(smallest(200, 2, 60));
console.log(divCount(200, 60));
console.log(topSmallest(40));
console.log(bigDivisors(parseInt(process.argv[3] ?? "0", 10)));
console.log(stepCount(300, 50));
console.log(gcdSum(3000, 123456));
console.log(
  rems(
    [0, -0, 7, -7, 9007199254740991, -9007199254740991, 9007199254740992, 7.5, -7.5, NaN, Infinity, -Infinity],
    [1, -1, 2, 3, 0, -0, 7.5, 9007199254740993, NaN, Infinity, -Infinity],
  ),
);
