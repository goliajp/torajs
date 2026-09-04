// §11.2.2's second source of strictness, reached through a FUNCTION's
// own directive prologue rather than the program's: `strictBody` below
// is strict mode code, so a direct eval in its body looks at strict
// code (§19.2.1.1 step 8) even though the file — a `.cts`, the sloppy
// script goal — is not strict anywhere else.
//
// bun disagrees on this one shape: it admits all six texts here, and
// `eval("this")` in the same body answers a non-undefined value, so it
// is not treating the prologue as arming the body at all. §11.2.2 and
// §19.2.1.1 are unambiguous, so the spec is the oracle and this file
// checks against a `.expected` rather than against bun.

function strictBody() {
  "use strict";
  // §12.7.2 — `yield` is a reserved word in strict code.
  try {
    eval("var yield = 1");
    console.log("s-yield ran");
  } catch (e) {
    console.log("s-yield", SyntaxError.prototype.isPrototypeOf(e));
  }
  // §12.7.2 — the future reserved words.
  try {
    eval("var static = 1");
    console.log("s-reserved ran");
  } catch (e) {
    console.log("s-reserved", SyntaxError.prototype.isPrototypeOf(e));
  }
  // annexB §B.1.1 / §B.1.2 — legacy octal.
  try {
    eval("var x = 010");
    console.log("s-octal ran");
  } catch (e) {
    console.log("s-octal", SyntaxError.prototype.isPrototypeOf(e));
  }
  // annexB §B.3.2 / §B.3.4 — a handed-back position.
  try {
    eval("if (true) function g(){}");
    console.log("s-annexb ran");
  } catch (e) {
    console.log("s-annexb", SyntaxError.prototype.isPrototypeOf(e));
  }
  // §15.1.2 — duplicate parameters.
  try {
    eval("function g(a, a){}");
    console.log("s-dup ran");
  } catch (e) {
    console.log("s-dup", SyntaxError.prototype.isPrototypeOf(e));
  }
  // §14.11.1 — `with`.
  try {
    eval("with ({}) {}");
    console.log("s-with ran");
  } catch (e) {
    console.log("s-with", SyntaxError.prototype.isPrototypeOf(e));
  }
  console.log("plain", eval("40 + 2"));
}
strictBody();

// The same body WITHOUT the prologue inherits nothing — this file's
// goal is sloppy, so every one of them is admitted again.
function sloppyBody() {
  try {
    eval("var yield = 1");
    console.log("p-yield ran");
  } catch (e) {
    console.log("p-yield", SyntaxError.prototype.isPrototypeOf(e));
  }
  try {
    eval("var static = 1");
    console.log("p-reserved ran");
  } catch (e) {
    console.log("p-reserved", SyntaxError.prototype.isPrototypeOf(e));
  }
  try {
    eval("var x = 010");
    console.log("p-octal ran");
  } catch (e) {
    console.log("p-octal", SyntaxError.prototype.isPrototypeOf(e));
  }
  try {
    eval("if (true) function g(){}");
    console.log("p-annexb ran");
  } catch (e) {
    console.log("p-annexb", SyntaxError.prototype.isPrototypeOf(e));
  }
  try {
    eval("function g(a, a){}");
    console.log("p-dup ran");
  } catch (e) {
    console.log("p-dup", SyntaxError.prototype.isPrototypeOf(e));
  }
  try {
    eval("with ({}) {}");
    console.log("p-with ran");
  } catch (e) {
    console.log("p-with", SyntaxError.prototype.isPrototypeOf(e));
  }
}
sloppyBody();
