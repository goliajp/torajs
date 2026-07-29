// RFC 20260729-fn-value-any V4 刀 3 — a `var` binding is an `any`
// slot whatever the source says (the hoist pass mints it that way on
// purpose: a pre-init read is `undefined`, and a var may be reassigned
// across types). The fn-value wrap collector read only the written
// annotation, so `var b = foo` — the plainest fn-value alias there is
// — rejected the whole program at box_to_any while the `let` / `const`
// forms had always worked.
function foo(): number {
  return 1;
}

var bare = foo;
console.log("call", bare());
console.log("typeof", typeof bare);
console.log("name", bare.name);

// Inside a fn body — the hoist walks each FnDecl body separately.
function inner(): void {
  var local = foo;
  console.log("inner", local(), typeof local);
}
inner();

// The collector recurses into object-literal fields and array
// elements of an any-destined init.
var obj = { cb: foo };
console.log("objlit", obj.cb());
var arr = [foo, foo];
console.log("arrlit", arr[1]());

// An explicit annotation on a `var` is a hint, not a slot constraint —
// the slot is still any, so the wrap must fire here too.
var annotated: any = foo;
console.log("annotated", annotated());

// NOT covered here: reassigning such a binding across types
// (`var poly = foo; poly = 7`) still rejects with a checker type
// mismatch — a PRE-EXISTING face this knife merely uncovered (the
// `let` form rejects identically on the clean tree, verified by a
// same-HEAD A/B), registered separately.
