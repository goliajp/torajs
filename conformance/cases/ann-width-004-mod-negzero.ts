// W3 (rfc 20260611-ann-width-unification §5.3) — `%` -0 semantics.
// JS spec §13.10: the remainder takes the dividend's sign, so a
// negative dividend with zero remainder yields -0 — observable
// through 1/x and console.log. i64 srem cannot represent it.

// R6 form: negative dividend, zero remainder → -0 through a call ret
function negParity(k: number): number {
  return k % 2;
}
console.log(negParity(-100));
console.log(1 / negParity(-100));
console.log(negParity(-99));

// negative dividend, nonzero remainder stays a plain negative int
console.log(-100 % 7);

// non-negative constant dividend keeps the int path (no -0 possible)
console.log(10 % 3);
console.log(0 % 5);

// divisor zero → NaN (constant and slot-loaded forms)
console.log(7 % 0);
let z: number = 0;
let seven: number = 7;
console.log(seven % z);

// comparison context: -0 is unobservable through ===, parity loops
// over negative values must count correctly
function evenCount(lo: number, hi: number): number {
  let c = 0;
  for (let i = lo; i <= hi; i++) {
    if (i % 2 === 0) c++;
  }
  return c;
}
console.log(evenCount(-7, 7));

// gcd shape: %-fed slot chain through a loop, integral results
function gcd(a: number, b: number): number {
  while (b !== 0) {
    const t = b;
    b = a % b;
    a = t;
  }
  return a;
}
console.log(gcd(1071, 462));
console.log(gcd(123456, 1234567));
