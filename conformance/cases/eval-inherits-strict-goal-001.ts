// §19.2.1.1 step 8 — a DIRECT eval's code is strict mode code when the
// CALLING code is, and §11.2.2's first source of strictness is the
// goal: a module is strict code, so every direct eval in this file
// looks at strict code however innocent its own text.
//
// The five families below used to reach only whole-program gates, and
// those gates run BEFORE the eval desugar inlines anything — so none of
// them ever saw eval text. The refusal is a THROW at the call site, not
// a compile-time reject: §19.2.1.1 step 12 raises the SyntaxError when
// the eval is evaluated, which the dead-code case at the bottom pins.

// §12.7.2 — `yield` is a reserved word in strict code.
try {
  eval("var yield = 1");
  console.log("yield ran");
} catch (e) {
  console.log("yield", SyntaxError.prototype.isPrototypeOf(e));
}
// §12.7.2 — the future reserved words.
try {
  eval("var static = 1");
  console.log("reserved ran");
} catch (e) {
  console.log("reserved", SyntaxError.prototype.isPrototypeOf(e));
}
// annexB §B.1.1 / §B.1.2 — legacy octal.
try {
  eval("var x = 010");
  console.log("octal ran");
} catch (e) {
  console.log("octal", SyntaxError.prototype.isPrototypeOf(e));
}
// annexB §B.3.2 / §B.3.4 — the two positions those productions hand a
// FunctionDeclaration back to, both "only when parsing code that is
// not strict mode code".
try {
  eval("if (true) function f(){}");
  console.log("annexb-if ran");
} catch (e) {
  console.log("annexb-if", SyntaxError.prototype.isPrototypeOf(e));
}
try {
  eval("l1: function f(){}");
  console.log("annexb-label ran");
} catch (e) {
  console.log("annexb-label", SyntaxError.prototype.isPrototypeOf(e));
}
// §15.1.2 — duplicate parameters.
try {
  eval("function f(a, a){}");
  console.log("dup-param ran");
} catch (e) {
  console.log("dup-param", SyntaxError.prototype.isPrototypeOf(e));
}
// §14.11.1 — `with`.
try {
  eval("var o = {}; with (o) {}");
  console.log("with ran");
} catch (e) {
  console.log("with", SyntaxError.prototype.isPrototypeOf(e));
}

// Ordinary eval text is unaffected.
console.log("kept", eval("40 + 2"));

// §19.2.1.1 step 12 is a RUNTIME error: an eval that never runs raises
// nothing at all.
if (false) {
  eval("var yield = 1");
}
console.log("dead-code-fine");
