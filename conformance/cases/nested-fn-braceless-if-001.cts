// Annex B §B.3.2 shape `if (c) function f() {}` INSIDE a fn body —
// the fn-body walk previously hit the lowering catch-all panic
// (module-top shape already lifted). §B.3.3 gives the branch
// declaration TWO bindings in sloppy code: the block-scoped one, and a
// var binding on the enclosing function scope written when the branch
// runs. The outer read finds that one.
//
// The S5.7 boundary this file used to record — "the sloppy-mode hoist
// face" — is decided here, and the decision is the spec's: node answers
// `1` / `42`, bun answers a caught ReferenceError because it has no
// sloppy goal to apply Annex B under. Hence a `.expected`, not bun.

// 1. if/else both carrying bare fn decls; the outer read finds the
//    Annex B var binding
function outer() {
  if (true) function f() { return 1; } else function g() { return 2; }
  try { return f(); } catch (e: any) { return "caught:" + (e instanceof ReferenceError); }
}
console.log(outer());

// 2. the common annexB shape: under an IIFE
const r = (function () {
  if (true) function h() { return 42; }
  try { return h(); } catch (e: any) { return "caught"; }
})();
console.log(r);

// 3. compile survives a bare decl under a loop body
function loops() {
  let n = 0;
  while (n < 1) { n = n + 1; if (true) function w() { return 7; } }
  return 3;
}
console.log(loops());
