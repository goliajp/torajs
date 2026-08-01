// Annex B §B.3.2 shape `if (c) function f() {}` INSIDE a fn body —
// the fn-body walk previously hit the lowering catch-all panic
// (module-top shape already lifted). TS semantics: the branch decl
// is block-scoped, so an outer reference throws a catchable
// ReferenceError (bun ground truth; the sloppy-mode hoist face is
// an S5.7 boundary decision, recorded).

// 1. if/else both carrying bare fn decls; outer read throws
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
