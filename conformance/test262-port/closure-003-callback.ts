// Adapted from test262: passing a function value as a parameter.
function apply(f: (n: number) => number, x: number): number {
  return f(x);
}

function double(n: number): number { return n * 2; }
function inc(n: number): number { return n + 1; }

function check(): number {
  if (apply(double, 5) !== 10) { throw "#1"; }
  if (apply(inc, 5) !== 6) { throw "#2"; }
  if (apply(double, 0) !== 0) { throw "#3"; }
  if (apply(inc, -1) !== 0) { throw "#4"; }
  return 0;
}
console.log(check());
