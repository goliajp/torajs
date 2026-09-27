// The value properties of Math and Number are doubles. Stored into a
// number binding read from a function, the slot was an integer one
// and printed the f64's bit pattern as an integer.
const pi: number = Math.PI;
let e: number = Math.E;
const tau: number = Math.PI * 2;
const eps: number = Number.EPSILON;
const inf: number = Number.POSITIVE_INFINITY;
const nan: number = Number.NaN;
const big: number = Number.MAX_SAFE_INTEGER;

function report(): void {
  console.log(pi, e, tau);
  console.log(eps, inf, nan, big);
  e = e + Math.SQRT2;
  const local: number = Math.LN2;
  console.log(e, local);
}

report();
console.log(pi + 1, e);
