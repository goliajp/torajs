// Annex B §B.3.3 — the block binding is a binding of its own: mutable,
// and independent of the var binding that shares its name.
//
// test262's `block-scoping` family writes it from inside the
// declaration's own body (`f = 123`) and then checks that the var
// binding OUTSIDE still holds the function. That needs three things at
// once: the block binding exists, it is writable, and the write does
// not reach the var one.
//
// A declaration whose own name is assigned is rewritten by
// `widen_rebound_fn_decls` before the Annex B pass ever sees it, so it
// used to lose the var binding entirely — `ReferenceError: g is not
// defined` after the block.
//
// The last case is the free-variable half of the same fix: `var` is
// function-scoped, so a declaration inside a block is bound after the
// block too. The walk used to drop it on the way out, and a closure
// around it captured a name its own body declares.
//
// bun is not an oracle here — it reads every file as strict code, where
// there is no var binding at all. node follows the spec.

var initialBV, currentBV, varBinding;
(function () {
  { function f() { initialBV = f; f = 123; currentBV = f; return "decl"; } }
  varBinding = f;
  f();
})();
console.log("fn", initialBV(), currentBV, varBinding());

var iG, cG;
{ function g() { iG = g; g = 456; cG = g; return "g-decl"; } }
g();
console.log("top", iG(), cG, g());

// The write inside the block does not reach the var binding, and the
// var binding is what a call after the block means.
(function () {
  { function h() { h = "gone"; return "h-decl"; } }
  console.log("independent", h(), typeof h);
})();

// `var` written inside a block is the enclosing function's binding.
(function () {
  { var q = 1; }
  console.log("var-in-block", q);
})();
