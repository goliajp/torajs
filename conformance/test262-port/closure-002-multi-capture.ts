// Adapted from test262: arrow-function capturing multiple outer bindings.
function makeLine(slope: number, intercept: number): (x: number) => number {
  return (x: number): number => slope * x + intercept;
}

function check(): number {
  let f = makeLine(2, 3);
  if (f(0) !== 3) { throw "#1"; }
  if (f(1) !== 5) { throw "#2"; }
  if (f(10) !== 23) { throw "#3"; }
  let g = makeLine(0, 7);
  if (g(100) !== 7) { throw "#4"; }
  return 0;
}
console.log(check());
