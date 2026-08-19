// §15.2.1 via AssignmentTargetType — an assignment or update targeting
// `eval` / `arguments` inside STRICT eval code is an early error,
// raised when the eval is evaluated (§19.2.1.1 step 12), catchable,
// never a compile error. The parenthesized spelling is the same
// reference (§13.2.9.3).

try {
  eval("eval = 42;");
  console.log("a1-ok");
} catch (e) {
  console.log("a1-threw", e instanceof SyntaxError);
}

// the site inside a function expression in the eval text — the whole
// text is refused before any of it runs
try {
  eval("var f = function () { eval = 42; };");
  console.log("a2-ok");
} catch (e) {
  console.log("a2-threw", e instanceof SyntaxError);
}

try {
  eval("function foo() { arguments = 1; }; foo();");
  console.log("a3-ok");
} catch (e) {
  console.log("a3-threw", e instanceof SyntaxError);
}

// strict-prologue Function body: creation-time SyntaxError
try {
  Function("'use strict'; eval = 42;");
  console.log("a4-ok");
} catch (e) {
  console.log("a4-threw", e instanceof SyntaxError);
}

// update expression targets the same reference
try {
  eval("eval++;");
  console.log("a5-ok");
} catch (e) {
  console.log("a5-threw", e instanceof SyntaxError);
}

// parenthesized reference is itself a reference
try {
  eval("(eval) = 42;");
  console.log("a6-ok");
} catch (e) {
  console.log("a6-threw", e instanceof SyntaxError);
}

try {
  eval("(arguments) = 1;");
  console.log("a7-ok");
} catch (e) {
  console.log("a7-threw", e instanceof SyntaxError);
}
