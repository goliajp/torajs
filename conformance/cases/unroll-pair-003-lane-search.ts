// Divisibility loops searched several divisors at a time after a few
// steps of the loop itself, which takes back the group that runs past
// the bound: the divisor found in every lane position (searches
// starting at 2, 3 and 4 over n < 230; 221 = 13 * 17 is the one whose
// first divisor lands in the third lane), a group whose last divisor fails the
// bound, squares and primes up to 2^31 - 1, dividends 0..3, negative
// and fractional dividends, the counter read after `break` and after
// the bound fails mid-group, a dividing lane past a bound on the
// counter, and a loop writing a second cell, which is not searched.
// The group the bound cuts: every bound from 2 to 41 against every
// dividend below 90, so the bound fails at each lane of the group and
// the first divisor falls before, at and after that lane, each with
// the counter read after the loop, for bounds `d <= b` and `b > d`.

const lim = parseInt(process.argv[2] ?? "230", 10);
const top = parseInt(process.argv[3] ?? "2147483648", 10);
const big = parseInt(process.argv[4] ?? "30000", 10);

// smallest divisor >= 2, the dividend itself when there is none
function smallestFrom2(n: number): number {
  let d = 2;
  while (d * d <= n) {
    if (n % d === 0) return d;
    d = d + 1;
  }
  return n;
}

// from 3: the lanes shift by one against the search from 2
function smallestFrom3(n: number): number {
  let d = 3;
  while (d * d <= n) {
    if (n % d === 0) return d;
    d = d + 1;
  }
  return 0;
}

// from 4: and by two
function smallestFrom4(n: number): number {
  let d = 4;
  while (d * d <= n) {
    if (n % d === 0) return d;
    d = d + 1;
  }
  return 0;
}

// the counter after the loop: a `break`, or the bound failing
function stepsTaken(n: number): number {
  let i = 2;
  while (i * i <= n) {
    if (n % i === 0) break;
    i = i + 1;
  }
  return i;
}

// a second cell written each step: the loop is not searched
function stepsCounted(n: number): number {
  let s = 0;
  let d = 2;
  while (d * d <= n) {
    if (n % d === 0) break;
    s = s + 3;
    d = d + 1;
  }
  return s + d;
}

// a bound on the counter itself: for 61 the divisor 61 is past it
function smallestTo60(x: number): number {
  for (let d = 2; d <= 60; d++) {
    if (x % d === 0) return d;
  }
  return 0;
}

// dividends at the top of the int range, 2147483647 prime
function smallestTop(n: number): number {
  let d = 2;
  while (d * d <= n) {
    if (n % d === 0) return d;
    d = d + 1;
  }
  return n;
}

// the bench shape, a whole run
function isPrime(n: number): boolean {
  if (n < 2) return false;
  let i = 2;
  while (i * i <= n) {
    if (n % i === 0) return false;
    i = i + 1;
  }
  return true;
}

// the counter after the loop, a bound on the counter given at run time
function stepsUpTo(x: number, b: number): number {
  let d = 2;
  while (d <= b) {
    if (x % d === 0) break;
    d = d + 1;
  }
  return d;
}

// the bound on the left and strict
function stepsBelow(x: number, b: number): number {
  let d = 2;
  while (b > d) {
    if (x % d === 0) return -d;
    d = d + 1;
  }
  return d;
}

// negative and fractional dividends take the float path
function smallestAny(n: number): number {
  let d = 2;
  while (d * d <= n) {
    if (n % d === 0) return d;
    d = d + 1;
  }
  return n;
}

let a = "";
let b = "";
let c = "";
let e = "";
let g = "";
let n = 0;
while (n < lim) {
  a += smallestFrom2(n) + ",";
  b += smallestFrom3(n) + ",";
  c += smallestFrom4(n) + ",";
  e += stepsTaken(n) + ",";
  g += stepsCounted(n) + ",";
  n = n + 1;
}
console.log(a);
console.log(b);
console.log(c);
console.log(e);
console.log(g);

let h = "";
let x = 50;
while (x < lim - 130) {
  h += smallestTo60(x) + ",";
  x = x + 1;
}
console.log(h);

let t = "";
let m = top - 48;
while (m < top) {
  t += smallestTop(m) + ",";
  m = m + 1;
}
console.log(t);

let primes = 0;
let k = 0;
while (k < big) {
  if (isPrime(k)) primes = primes + 1;
  k = k + 1;
}
console.log(primes);

let f = "";
for (let y = -60.5; y < 60; y += 3) f += smallestAny(y) + ",";
for (let y = -60; y < 60; y += 7) f += smallestAny(y) + ",";
console.log(f);

let u = "";
for (let bound = 2; bound < 42; bound++) {
  let w = "";
  for (let z = 0; z < 90; z++) w += stepsUpTo(z, bound) + "," + stepsBelow(z, bound) + ",";
  u += w + "\n";
}
console.log(u);
