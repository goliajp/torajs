// The sloppy-code half of `fn-body-var-over-fn-001.ts`: a `var` and a
// `function` of one name share one binding (§10.2.11 steps 27-28 in a
// function body, §16.1.7 steps 8 / 10 in a script), here reached
// through the two places only a Script has — the script's own top
// level, and a sloppy direct eval, whose declarations land in the
// calling function's variable environment (§19.2.1.3 steps 13-16).
// Oracle: node running it as a Script; bun answers the same here.

// script top, init-less `var` and a later assignment
var z;
function z() { return 1; }
var zBefore = typeof z;
z = 3;
console.log("top-assign", zBefore, z);

// eval text declares both; the caller's binding is the shared one
function evalInit() {
  return eval("function y() { return 7; } var y = 2; typeof y");
}
console.log("eval-init", evalInit());

function evalLeak() {
  eval("var y = 1; function y() { return 8; }");
  return typeof y + ":" + y;
}
console.log("eval-leak", evalLeak());

function evalNoInit() {
  eval("var y; function y() { return 9; }");
  return y();
}
console.log("eval-no-init", evalNoInit());

// the eval's `var` over the caller's own declaration
function evalOverOwn() {
  function w() { return "own"; }
  var before = w();
  eval("var w = 'eval';");
  return before + "," + w;
}
console.log("eval-over-own", evalOverOwn());
