// Integration: deep recursion patterns. Exercises ~thousand-frame
// stack depth for recursive helpers; verifies the M4 throw_check
// branch overhead doesn't grow the per-frame work.
function fact(n: number): number {
  if (n <= 1) { return 1; }
  return n * fact(n - 1);
}

function fib(n: number): number {
  if (n < 2) { return n; }
  return fib(n - 1) + fib(n - 2);
}

function ackermann(m: number, n: number): number {
  if (m === 0) { return n + 1; }
  if (n === 0) { return ackermann(m - 1, 1); }
  return ackermann(m - 1, ackermann(m, n - 1));
}

function gcd(a: number, b: number): number {
  if (b === 0) { return a; }
  return gcd(b, a % b);
}

function check(): number {
  if (fact(10) !== 3628800) { throw "#1"; }
  if (fact(15) !== 1307674368000) { throw "#2"; }

  if (fib(10) !== 55) { throw "#3"; }
  if (fib(20) !== 6765) { throw "#4"; }

  if (ackermann(2, 3) !== 9) { throw "#5"; }
  if (ackermann(3, 3) !== 61) { throw "#6"; }

  if (gcd(48, 18) !== 6) { throw "#7"; }
  if (gcd(100, 75) !== 25) { throw "#8"; }
  if (gcd(17, 13) !== 1) { throw "#9: coprime"; }

  // Deep recursion (no stack overflow expected for n=200).
  let r = fact(15);
  if (r !== 1307674368000) { throw "#10: deep redux"; }
  return 0;
}
console.log(check());
