// Adapted from test262: a fn returning one of its callable args. The
// returned value is a fn pointer, callable by the outer scope. (3-deep
// curry escape would need nested fn-type-alias annotation which tr's
// type-decl parser doesn't accept yet.)
function pick(b: boolean, f: (n: number) => number, g: (n: number) => number): (n: number) => number {
  if (b) { return f; }
  return g;
}

function inc(n: number): number { return n + 1; }
function dbl(n: number): number { return n * 2; }

function check(): number {
  let h = pick(true, inc, dbl);
  if (h(5) !== 6) { throw "#1"; }
  let k = pick(false, inc, dbl);
  if (k(5) !== 10) { throw "#2"; }
  return 0;
}
console.log(check());
