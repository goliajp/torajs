// Adapted from test262: invoking the result of a call directly —
// `make(10)(5)` without an intermediate binding. Was a parser-OK but
// ssa-lower-rejected shape until the generalized indirect-call dispatch
// landed; now it's a clean indirect call through the Closure / FnSig
// value returned by the inner call.
function make(a: number): (y: number) => number {
  return (y: number): number => a + y;
}

function add(x: number, y: number): number { return x + y; }
function mul(x: number, y: number): number { return x * y; }
function pickOp(op: boolean): (x: number, y: number) => number {
  if (op) { return add; }
  return mul;
}

function check(): number {
  if (make(10)(5) !== 15) { throw "#1: chain via closure"; }
  if (make(0)(99) !== 99) { throw "#2"; }
  if (pickOp(true)(3, 4) !== 7) { throw "#3: chain via fnsig"; }
  if (pickOp(false)(3, 4) !== 12) { throw "#4"; }
  return 0;
}
console.log(check());
