// A function declared to return `(...) => R` may return a mix of
// shapes across branches: capturing arrows (Closure-shape at the SSA
// layer) and bare top-level fn names (FnSig-shape). Without
// reconciliation, the call site's CallIndirect dispatch picks one
// convention and the other branch SIGSEGVs.
//
// `synthesize_forwarders` (AST pass after lift_arrow_fns) detects the
// mixed shape and rewrites each `return some_fn;` into
// `return Closure { fn_name: "__forward_some_fn", captures: [] }`,
// where the synthesized forwarder takes a `__env` first param
// (matching closure calling convention) and forwards to the wrapped
// fn. Both branches now produce uniform Closure values.

function add5(y: number): number { return y + 5; }

function pickFn(useCapturing: boolean, base: number): (y: number) => number {
  if (useCapturing) {
    return (y: number): number => base + y;  // capturing arrow → Closure
  }
  return add5;  // bare fn → wrapped via synthesized __forward_add5
}

function check(): number {
  let f = pickFn(true, 10);
  if (f(3) !== 13) { throw "#1 capturing"; }   // 10 + 3
  if (f(0) !== 10) { throw "#2 capturing rebind"; }
  let g = pickFn(false, 0);
  if (g(7) !== 12) { throw "#3 forwarder"; }   // 7 + 5 via add5
  if (g(20) !== 25) { throw "#4 forwarder again"; }
  return 0;
}
console.log(check());
