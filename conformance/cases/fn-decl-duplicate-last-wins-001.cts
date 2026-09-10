// §16.1.7 GlobalDeclarationInstantiation / §10.2.11 — duplicate
// `function` declarations in a Script: the LAST declaration is the
// one instantiated, and hoisting makes it the binding's value from
// the first statement on. The checker registered declarations in
// order and refused the second as a redeclaration; on the Script
// goal `dedupe_fn_decls` keeps the last per name (top level and
// function bodies) before the checker looks. bun runs the file as a
// script and is the oracle.
function f() { return 1; }
var g = f;
var first = f();
function f() { return 2; }
console.log(f(), g(), first, g === f);
function outer() {
  function h() { return "a"; }
  var early = h();
  function h() { return "b"; }
  return [early, h()];
}
console.log(outer());
function three() { return 1; } function three() { return 2; } function three() { return 3; }
console.log(three());
function $DONE(e) { return "harness"; }
function $DONE(e) { return e === undefined ? "ok" : "err"; }
console.log($DONE(), $DONE(1));
