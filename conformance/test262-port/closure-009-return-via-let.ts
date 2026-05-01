// Adapted from test262: language/expressions/arrow-function/* — a factory
// fn that builds a closure into a local before returning it. Tr previously
// only detected `return <closureExpr>` (direct return); the let-then-return
// shape went via FnSig (raw fn ptr) ABI even though the value is a closure
// (env block), giving SIGBUS on call. The walker now collects closure-typed
// locals across the body and treats `return <ident>` as a closure return
// when the ident's let-init was a closure.
function makeAdder(n: number): (m: number) => number {
  let f = (m: number): number => m + n;
  return f;
}

function makeMul(n: number): (m: number) => number {
  let g = (m: number): number => m * n;
  return g;
}

function check(): number {
  let add5 = makeAdder(5);
  let add100 = makeAdder(100);
  if (add5(3) !== 8) { throw "#1"; }
  if (add5(0) !== 5) { throw "#2"; }
  if (add100(50) !== 150) { throw "#3"; }

  let mul3 = makeMul(3);
  if (mul3(7) !== 21) { throw "#4"; }
  if (mul3(0) !== 0) { throw "#5"; }

  // Independent factories — each closure carries its own captured `n`.
  if (add5(10) !== 15) { throw "#6"; }
  return 0;
}
console.log(check());
