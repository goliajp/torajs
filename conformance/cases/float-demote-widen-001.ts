// float_demote widens the 3n+1 window check of a collatz step from
// the odd arm to the parity test ahead of it, so an even value past
// the window leaves the i64 loop too; the f64 clone must restart that
// iteration from the parity test (halving it), not from the odd arm.
function steps(n: number): number {
  let count: number = 0;
  while (n !== 1) {
    if (n % 2 === 0) {
      n = n / 2;
    } else {
      n = 3 * n + 1;
    }
    count = count + 1;
  }
  return count;
}
// even and past the window on the first iteration
console.log(steps(2 ** 52));
console.log(steps(2 ** 52 + 2));
// odd past the window: 3n+1 leaves 2^53 and rounds in f64
console.log(steps(2 ** 52 + 1));
console.log(steps(3002399751580331));
// crosses the window partway through
console.log(steps(1501199875790165));
// small values stay in the i64 loop
let max = 0;
for (let i = 1; i < 3000; i++) {
  const s = steps(i);
  if (s > max) max = s;
}
console.log(max);

// the counter updates before the parity test: re-running the
// iteration in the f64 clone would count it twice, so this shape
// keeps the check on the odd arm
function stepsFirst(n: number): number {
  let count: number = 0;
  while (n !== 1) {
    count = count + 1;
    if (n % 2 === 0) {
      n = n / 2;
    } else {
      n = 3 * n + 1;
    }
  }
  return count;
}
console.log(stepsFirst(2 ** 52));
console.log(stepsFirst(2 ** 52 + 1));
console.log(stepsFirst(27));
