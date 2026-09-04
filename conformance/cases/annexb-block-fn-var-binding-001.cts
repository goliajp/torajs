// Annex B §B.3.3 — a `function` declaration nested in a block, in
// SLOPPY code, has TWO bindings: the block-scoped one, and a
// var-scoped one on the nearest function/global VariableEnvironment.
// The var binding is created on scope entry holding `undefined`, and
// written when the declaration is *evaluated* — that is, when control
// reaches its textual position (§B.3.3.1 step 3.a.ii).
//
// bun is not an oracle here: it reads every file as strict code, where
// there is no var binding at all, and answers `ReferenceError: f is not
// defined` for the first line of this file. node agrees with the spec
// on every line below, so this checks against a `.expected`.

// Read before the block runs: the binding exists and holds `undefined`.
console.log("pre", typeof a);
if (true) function a() { return "a"; }
console.log("post", typeof a, a());

// A braceless `if` clause and a real block are the same shape.
console.log("pre", typeof b);
{ function b() { return "b"; } }
console.log("post", typeof b, b());

// A switch clause is a block too.
switch (1) { case 1: function c() { return "c"; } }
console.log("switch", typeof c, c());

// An existing `var` of the same name keeps its value until the
// declaration is reached — the var binding is shared, not re-created.
var d = 123;
console.log("existing-var", d);
if (true) function d() { return "d"; }
console.log("existing-var", typeof d, d());

// Inside a function body, the var binding is that body's, not the
// module's.
function host() {
  console.log("fn pre", typeof e);
  { function e() { return "e"; } }
  console.log("fn post", typeof e, e());
}
host();
console.log("fn leaked", typeof e);

// The block binding is what a call inside the block reaches.
{
  function g() { return "g"; }
  console.log("in-block", g());
}
