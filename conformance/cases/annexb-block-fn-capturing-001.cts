// Annex B §B.3.3 for a block function that CAPTURES — the half that
// never had the var binding at all.
//
// A nested `function` that reads something from the scope around it
// does not take tr's lifting lane (a lifted body is checked in
// top-level scope, where the captured local does not exist); it is
// rewritten in place into a closure instead. That rewrite gave it the
// block binding and stopped there, so `{ function g() { i = 1; } } g()`
// answered `ReferenceError: g is not defined` while the same
// declaration WITHOUT a capture worked.
//
// Both bindings exist here as everywhere else in §B.3.3: the block one
// the closure rewrite makes, and a var-scoped one written where the
// declaration sits.
//
// bun is not an oracle here — it reads every file as strict code, where
// there is no var binding. node follows the spec, so this checks
// against a `.expected`.

// Script level: the var binding is the script's.
var seen;
{ function g() { seen = "ran"; return "g"; } }
console.log("top", g(), seen);

// Function body: the var binding is that body's.
(function () {
  var seen2;
  { function h() { seen2 = "ran"; return "h"; } }
  console.log("fn", h(), seen2);
})();

// The reference inside the block is the BLOCK binding, not the var one.
var got;
{ function k() { got = typeof k; return "k"; } }
console.log("self", k(), got);

// Read before the block runs: created on scope entry, holding undefined.
console.log("pre", typeof m);
var cap;
{ function m() { cap = "m"; return "m"; } }
console.log("post", typeof m, m(), cap);

// A braceless `if` clause is the same shape.
var n1;
if (true) function n() { n1 = "n"; return "n"; }
console.log("if", n(), n1);
