// 403-02 — a promoted this-reading fn-expr and a 1M-deep
// self-recursive named fn expression COEXIST: the self-recursive
// call is provably the enclosing (unpromoted) closure via its self
// slot, so it skips the recv gate and keeps the egraph
// self-tail-call shape. Pre-fix the gate's two-arm form broke the
// TCO match and the recursion overflowed the real stack.
class A {
  f: () => void;
  constructor() {
    this.f = function () { console.log("face", typeof this); };
  }
}
const a = new A();
const det = a.f;
det();

let callCount = 0;
(function f(n: any) {
  if (n === 0) {
    callCount += 1;
    return;
  }
  return f(n - 1);
})(1000000);
console.log(callCount);
