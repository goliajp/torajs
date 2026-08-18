// `delete <bare name>` inside eval / Function literal text — §13.5.1 per
// the goal of the TEXT, not the program. Strict eval code raises the
// §13.5.1.1 early error at EVALUATION time (§19.2.1.1 step 12), a
// sloppy Function body resolves the site per §13.5.1.2, and none of
// these may refuse the whole program at compile time.

// strict eval code: delete on a bare name is a SyntaxError when the
// eval is reached, catchable, not a compile error
try {
  eval("delete xzy;");
  console.log("e1-ok");
} catch (e) {
  console.log("e1-threw", e instanceof SyntaxError);
}

// Function body without a strict prologue is sloppy: the form is legal
Function("delete xzy;");
console.log("e2-ok");

// strict-prologue body: creation-time SyntaxError (§20.2.1.1 step 22)
try {
  Function("'use strict'; delete xzy;");
  console.log("e3-ok");
} catch (e) {
  console.log("e3-threw", e instanceof SyntaxError);
}

// §13.5.1.2 in the sloppy body: unresolvable name answers true,
// a declared binding answers false, `undefined` is non-configurable
console.log("e4", Function("return delete xzy;")());
console.log("e5", Function("var q = 1; return delete q;")());
console.log("e6", Function("return delete undefined;")());

// value-position eval throws the same way
try {
  var r = eval("delete xzy;");
  console.log("e7-ok", r);
} catch (e) {
  console.log("e7-threw", e instanceof SyntaxError);
}

// an unreached eval raises nothing (step 12 is evaluation-time)
if (false) {
  eval("delete xzy;");
}
console.log("e8-unreached-ok");
