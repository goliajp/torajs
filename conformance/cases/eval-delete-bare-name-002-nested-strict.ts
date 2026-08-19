// §11.2.1 — a 'use strict' directive prologue arms its FUNCTION BODY
// even when the outermost Function(...) text is sloppy: a `delete
// <bare name>` inside such a nested body is the §13.5.1.1 early error
// and fails the whole dynamic function at creation time, while the
// same form in the sloppy outer body stays legal.

// nested strict-prologue fn: creation-time SyntaxError
try {
  Function('function f() { "use strict"; delete xz; }');
  console.log("n1-ok");
} catch (e) {
  console.log("n1-threw", SyntaxError.prototype.isPrototypeOf(e));
}

// same nesting without the prologue: sloppy all the way down, legal
Function("function f() { delete xz; }");
console.log("n2-ok");

// fn expression nested in the sloppy body, strict prologue inside
try {
  Function('var g = function () { "use strict"; delete xz; };');
  console.log("n3-ok");
} catch (e) {
  console.log("n3-threw", SyntaxError.prototype.isPrototypeOf(e));
}

// arrow inherits the sloppy enclosure: still legal
console.log("n4", Function("var h = () => delete xz; return h();")());
