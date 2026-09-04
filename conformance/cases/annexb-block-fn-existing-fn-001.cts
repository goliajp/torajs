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

// Recorded boundary — the same collision one scope down does NOT work
// yet, and this file states only what does. Inside a FUNCTION body tr
// lifts the body's own `function h` out and renames it, so after the
// pass that scope binds no `h` for the Annex B write to update; the
// module scope keeps its declaration, which is why the shape above
// answers. RFC 20260904-annexb-block-fn-two-bindings, knife A2.
