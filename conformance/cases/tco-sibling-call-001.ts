// Direct calls in tail position: a block whose call result is the
// return value leaves through the callee, so deep tail recursion runs
// in constant stack and every other shape keeps its call.

function ackermann(m: number, n: number): number {
  if (m === 0) return n + 1;
  if (n === 0) return ackermann(m - 1, 1);
  return ackermann(m - 1, ackermann(m, n - 1));
}
console.log(ackermann(2, 3), ackermann(3, 3), ackermann(0, 0), ackermann(1, 0));

// one million frames deep: only a reused frame gets through
function countdown(n: number, acc: number): number {
  if (n === 0) return acc;
  return countdown(n - 1, acc + (n % 7));
}
console.log(countdown(1000000, 0));

// both arguments read the old values (parallel move)
function swapDown(a: number, b: number): number {
  if (a <= 0) return b * 1000 + a;
  return swapDown(b - 1, a);
}
console.log(swapDown(9, 4), swapDown(4, 9));

function gcd(a: number, b: number): number {
  if (b === 0) return a;
  return gcd(b, a % b);
}
console.log(gcd(1071, 462), gcd(17, 5), gcd(0, 9), gcd(-12, 18));

// a boolean parameter flips on every round
function parity(n: number, even: boolean): boolean {
  if (n === 0) return even;
  return parity(n - 1, !even);
}
console.log(parity(10, true), parity(7, true));

// void tail calls
let visits = 0;
function walk(n: number): void {
  visits = visits + 1;
  if (n === 0) return;
  return walk(n - 1);
}
walk(250000);
console.log(visits);

// the tail call sits in both arms of a conditional
function collatzSteps(n: number, steps: number): number {
  if (n === 1) return steps;
  if (n % 2 === 0) return collatzSteps(n / 2, steps + 1);
  return collatzSteps(3 * n + 1, steps + 1);
}
console.log(collatzSteps(27, 0), collatzSteps(1, 0));

// each round makes a closure over that round's parameter: the rounds
// must not share it
const seen: (() => number)[] = [];
function capture(n: number): number {
  seen.push(() => n * 10);
  if (n === 0) return seen.length;
  return capture(n - 1);
}
console.log(capture(4), seen.map((f) => f()).join(","));

// not a tail call: the result is used after the call returns
function sumTo(n: number): number {
  if (n === 0) return 0;
  return n + sumTo(n - 1);
}
console.log(sumTo(1000));

// a tail call inside try is not in tail position
function guarded(n: number): number {
  try {
    if (n === 0) throw new Error("bottom");
    return guarded(n - 1);
  } catch (e) {
    return n * 100;
  }
}
console.log(guarded(5));

// fractional, negative and NaN arguments travel unchanged
function halve(x: number, k: number): number {
  if (k === 0) return x;
  return halve(x / 2, k - 1);
}
console.log(halve(10, 3), halve(-0.5, 2), halve(NaN, 2), Object.is(halve(-0, 1), -0));
