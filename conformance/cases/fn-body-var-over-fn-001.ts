// §10.2.11 FunctionDeclarationInstantiation — a function body's own
// `function z` and a `var z` of the same body are ONE binding: the
// function declaration is a VarScopedDeclaration (§8.2.10
// TopLevelVarScopedDeclarations), so the pair is not the
// VarDeclaredNames × LexicallyDeclaredNames clash §15.2.1 forbids.
// Steps 27-28 create the binding once, step 36 initializes it to the
// function on entry; an init-less `var z;` leaves it alone and
// `var z = e` / `z = e` overwrite it when they run.

function noInitBefore() { var z; function z() { return 1; } return typeof z; }
console.log("no-init-before", noInitBefore());

function noInitAfter() { function z() { return 1; } var z; return typeof z; }
console.log("no-init-after", noInitAfter());

function callThrough() { var z; function z() { return 3; } return z(); }
console.log("call-through", callThrough());

function initBefore() { var z = 5; function z() { return 1; } return z; }
console.log("init-before", initBefore());

function initAfter() { function z() { return 1; } var z = 7; return typeof z + ":" + z; }
console.log("init-after", initAfter());

// The function's value is there from the first statement; the `var`'s
// initializer replaces it only where it runs.
function readFirst() {
  var a = typeof z;
  var b = z();
  var z = 5;
  function z() { return 2; }
  return a + "," + b + "," + z;
}
console.log("read-first", readFirst());

// a plain assignment over the declaration, no initializer anywhere
function assignOnly() { var z; function z() { return 1; } z = 3; return z; }
console.log("assign-only", assignOnly());

// the initializer runs in a nested block, the declaration stays hoisted
function inBlock(c: boolean) {
  var before = typeof z;
  if (c) { var z = [1, 2]; }
  function z() { return 1; }
  return before + "," + (typeof z === "function" ? "function" : z.length);
}
console.log("in-block", inBlock(true), inBlock(false));

// a closure over the shared binding sees the later write
function closure() {
  function z() { return 1; }
  var read = () => typeof z;
  var first = read();
  var z = "s";
  return first + "," + read();
}
console.log("closure", closure());

// a declaration that captures a local of its body
function capturing() {
  var k = 10;
  var z = 4;
  function z() { return k; }
  return z;
}
console.log("capturing", capturing());

// function-expression and arrow bodies are function bodies too
const expr = function () { var y = [1]; function y() { return 4; } return y; };
console.log("fn-expr", expr());
const arrow = () => { var y = function () { return 5; }; function y() { return 4; } return y(); };
console.log("arrow", arrow());

// one level deeper — a nested function's own body
function outer() {
  function inner() { var w = 8; function w() { return 1; } return w; }
  return inner();
}
console.log("nested", outer());

// eval text in strict code is its own variable environment (§19.2.1.1
// step 16), and inside it the pair is the same one binding
function viaEval() {
  return eval("var z = 2; function z() { return 7; } typeof z") + "," +
    eval("var z; function z() { return 7; } z()");
}
console.log("eval", viaEval());
