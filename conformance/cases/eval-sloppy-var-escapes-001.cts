// §19.2.1.1 steps 3-5 — a strict eval gets its OWN VariableEnvironment
// and its `var`s die with it; a SLOPPY one does not, so its `var` and
// its function declarations land in the caller's. tr sealed on the
// CALL FORM instead ("a direct eval is always strict"), which meant a
// sloppy direct eval declared into a scope nobody could see.
//
// This file is `.cts` — the sloppy script goal, and nothing in it is
// strict — so every declaration below reaches the enclosing scope.

eval("var v = 5;");
console.log("var", typeof v, v);

eval("function q(){ return 1; }");
console.log("fn", typeof q, q());

// A duplicate-parameter list is legal here (§15.1.2) and the function
// it declares is just as visible.
eval("function dup(a, a){ return a; }");
console.log("dup", dup(1, 2));

// Inside a function the eval declares into THAT function's variable
// environment, not the program's.
function host() {
  eval("var inner = 7;");
  return typeof inner + ":" + inner;
}
console.log("nested", host());
console.log("not-leaked", typeof inner);

// A "use strict" prologue in the text arms the seal from the other
// side: these bindings die with the eval whatever the caller was.
eval("'use strict'; var sealed = 9; function sq(){ return 1; }");
console.log("sealed", typeof sealed, typeof sq);
