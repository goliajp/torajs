// JS uses f64 numbers, and `n & 1` / `n >> 1` first truncate to int32 — for
// collatz starting values up to 1_000_000, the trajectory peaks reach
// ~57 billion (≈ 2^36), which would wrap under int32 ops and loop forever.
// Use `% 2` and `/ 2` instead — both stay exact in f64 up to 2^53.
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

let max: number = 0;
let i: number = 1;
while (i <= 1000000) {
  const s: number = steps(i);
  if (s > max) {
    max = s;
  }
  i = i + 1;
}
console.log(max);
