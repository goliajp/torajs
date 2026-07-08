// Chunk 701 — a runtime-helper pending throw (TLS-recorded, no AST
// `throw` node) must propagate across the named-fn call boundary.
// may_throw_fns previously had no arm for method calls, so the
// caller's throw-check was M4.3.b-skipped: try/catch printed "not
// reached", a non-void callee answered the ret sentinel (null here),
// and execution continued with exit code 0. normalize("NFX") is the
// probe surface — spec RangeError on both engines (ES §22.1.3.14).
function norm(s: string, form: string): string {
  return s.normalize(form);
}
try {
  console.log(norm("a", "NFC"));
  console.log(norm("a", "NFX"));
  console.log("not reached");
} catch (e) {
  console.log("caught");
}
// transitive: the throw bit propagates through the fixed-point —
// outer has no method call of its own, only the call to inner
function inner(form: string): string {
  return "b".normalize(form);
}
function outer(form: string): string {
  return inner(form) + "!";
}
try {
  console.log(outer("NFC"));
  console.log(outer("nope"));
} catch (e) {
  console.log("caught transitive");
}
console.log("done");
