// RFC 20260912-global-var-cell knife 4-1 — `$262.evalScript(src)` (the
// runner spells it `__t262_evalScript`) is a Script (§16.1.7), not an
// eval: its lexical declarations are global, its `var`s are non-
// configurable properties, and a declaration collision is a SyntaxError
// thrown when the Script is evaluated, creating no bindings.
// oracle: node script goal with `__t262_evalScript = s => vm.runInThisContext(s)`,
// EXCEPT the "no bindings" line: V8 declares a Script's `var`s one by one
// and throws at the colliding one, so `var x1; var outerLet;` leaves `x1`
// declared. §16.1.7 runs every step-3/4 check before any binding is
// created (test262 `script-decl-var-collision` asserts exactly this), so
// the expected holds the spec's answer: all seven are ReferenceError.
// Registered residue: a Script INSIDE a function that declares a new
// global function (`function outerVar() {}`) is not inlined and is not
// in this fixture.
let outerLet = 1;
var outerVar = 2;
function outerFn() {}
__t262_evalScript('let s1 = 10; const s2 = 20; class S3 {}');
s1 = 11;
console.log("lex", s1, s2, typeof S3, this.hasOwnProperty("s1"), this.hasOwnProperty("s2"));
__t262_evalScript('var sv = 5;');
var d = Object.getOwnPropertyDescriptor(this, "sv");
console.log("var", sv, d.value, d.writable, d.enumerable, d.configurable);
__t262_evalScript('function sf() { return 7; }');
console.log("fn", sf(), typeof this.sf);
function attempt(f) {
  try { f(); return "ok"; } catch (e) { return e instanceof SyntaxError ? "SyntaxError" : String(e); }
}
console.log("var-on-let", attempt(function () { __t262_evalScript('var x1; var outerLet;'); }));
console.log("let-on-var", attempt(function () { __t262_evalScript('var x2; let outerVar;'); }));
console.log("let-on-fn", attempt(function () { __t262_evalScript('var x3; let outerFn;'); }));
console.log("let-on-let", attempt(function () { __t262_evalScript('var x4; let outerLet;'); }));
console.log("fn-on-let", attempt(function () { __t262_evalScript('var x5; function outerLet() {}'); }));
console.log("let-on-script-let", attempt(function () { __t262_evalScript('var x6; let s1;'); }));
console.log("var-on-var", attempt(function () { __t262_evalScript('var outerVar;'); }));
console.log("eval-var-on-let", attempt(function () { (0, eval)('var x7; var outerLet;'); }));
console.log("eval-var-on-var", attempt(function () { (0, eval)('var outerVar;'); }));
function refs(f) {
  try { f(); return "declared"; } catch (e) { return e instanceof ReferenceError ? "ReferenceError" : String(e); }
}
console.log("no bindings", refs(function () { x1; }), refs(function () { x2; }), refs(function () { x3; }),
  refs(function () { x4; }), refs(function () { x5; }), refs(function () { x6; }), refs(function () { x7; }));
console.log("after", outerLet, typeof outerVar, typeof outerFn);
