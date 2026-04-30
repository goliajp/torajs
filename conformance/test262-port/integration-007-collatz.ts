// Integration: Collatz step count. Avoids `/` (which always returns
// f64 in tr — assigning f64 back to an i64 slot silently corrupts the
// bit pattern) by using `>>` for the integer-divide-by-2 step.
function collatz(n: number): number {
  let steps: number = 0;
  while (n !== 1) {
    if (n % 2 === 0) {
      n = n >> 1;
    } else {
      n = 3 * n + 1;
    }
    steps = steps + 1;
  }
  return steps;
}

function check(): number {
  if (collatz(1) !== 0) { throw "#1"; }
  if (collatz(6) !== 8) { throw "#2"; }      // 6 → 3 → 10 → 5 → 16 → 8 → 4 → 2 → 1
  if (collatz(27) !== 111) { throw "#3"; }   // famous long chain
  return 0;
}
console.log(check());
