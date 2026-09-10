// §20.2.1.1 CreateDynamicFunction in STATEMENT position, where the
// created function is discarded. An unannotated parameter is an
// implicit generic, so the read of the synthesized decl escapes as a
// value with no mono channel — it died with "unknown ident
// `__dynfn_0`" where bun evaluates the statement and moves on. The
// duplicate-parameter shapes are the §20.2.1.1 test262 family.
Function("a", "return;");
Function("a", "a", "return;");
Function("a", "b", "a", "return;");
console.log("created");

// The same escape without the Function ctor: a generic fn read as a
// bare expression statement.
function q(a) {
  return a;
}
q;
console.log(q(7));

// The consuming shapes must keep working: call, `var` init, typeof.
console.log(Function("a", "a", "return a;")(1, 2), typeof Function("a", "return;"));
const f = Function("a", "a", "return;");
console.log(f.length);
