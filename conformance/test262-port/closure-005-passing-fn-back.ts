// Adapted from test262: combinator pattern — apply twice via the same
// closure value passed as both arg and result driver.
function twice(f: (n: number) => number, x: number): number {
  return f(f(x));
}

function inc(n: number): number { return n + 1; }
function dbl(n: number): number { return n * 2; }

function check(): number {
  if (twice(inc, 5) !== 7) { throw "#1"; }
  if (twice(dbl, 3) !== 12) { throw "#2"; }
  if (twice(inc, 0) !== 2) { throw "#3"; }
  return 0;
}
console.log(check());
