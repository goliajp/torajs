// Annex B §B.3.3 step 3.a.ii.1 — the var binding is created and set to
// `undefined` only when the variable environment does not already have
// one. A scope that declares `function f` at its own top already has
// it, so the Annex B write updates that binding instead of shadowing
// it with a fresh `undefined` one.
//
// Same oracle note as `annexb-block-fn-var-binding-001`: bun reads this
// file as strict code and answers ReferenceError, node agrees with the
// spec, so the check is against a `.expected`.

function f() { return "outer"; }
console.log("before", f());
if (true) function f() { return "inner"; }
console.log("after", f());

// A declaration STILL AHEAD is a different case: it is hoisted over
// anything written before it, so §B.3.3 is skipped and the lift answers
// as it did before — which is the same answer. What matters here is
// that the else-branch declaration beside it is NOT skipped, so this
// one parent scope mints an Annex B write and a legacy lift at once.
// That is the only shape in which the rewrite half runs over a
// statement the lift just wrote.
if (true) function g() { return "g inner"; } else function unused() {}
console.log("ahead", typeof g, g());
function g() { return "g outer"; }

// The same collision one scope down. A function body's own `function h`
// does not survive the lift — it is renamed out — so the write would
// have nothing to update. When the body ALSO declares the name in a
// block, the body-level declaration becomes a hoisted `var` of its own,
// which is where a function declaration's initialization belongs
// anyway; the block's write then updates that binding.
function host() {
  function h() { return "fn-outer"; }
  console.log("fn before", h());
  { function h() { return "fn-inner"; } }
  console.log("fn after", h());
}
host();
