// Top-tested loops whose exit test the rotation copies into the latch:
// zero-trip, one-trip, early exit through break / return, continue,
// and a nested pair sharing the outer counter.
function countDivisors(n: number): number {
  let c = 0;
  let i = 1;
  while (i * i <= n) {
    if (n % i === 0) c = c + (i * i === n ? 1 : 2);
    i = i + 1;
  }
  return c;
}
function firstFactor(n: number): number {
  let i = 2;
  while (i * i <= n) {
    if (n % i === 0) return i;
    i = i + 1;
  }
  return n;
}
let s = 0;
for (let n = 0; n < 50; n = n + 1) s = s + countDivisors(n);
console.log(s, firstFactor(1), firstFactor(49), firstFactor(97), firstFactor(0));
let k = 10;
while (k < 10) k = k + 1;
let t = 0;
for (let a = 0; a < 5; a = a + 1) {
  if (a === 2) continue;
  for (let b = a; b < 5; b = b + 1) {
    if (b === 4) break;
    t = t + a * b;
  }
}
let zero = 0;
let r = 0;
for (let d = 0; d < 3; d = d + 1) {
  if ((7 % d) === 0) r = r + 1;
  if ((zero % 5) === 0) r = r + 10;
}
console.log(k, t, r);
