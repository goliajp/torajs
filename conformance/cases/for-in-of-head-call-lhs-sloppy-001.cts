// Annex B §B.3.6.1 — in sloppy code a call expression is an
// admissible for-in / for-of head: the head is evaluated (the call
// runs), then PutValue on the non-reference throws a ReferenceError
// before the result is ever coerced (test262 annexB
// assignmenttargettype/callexpression-as-for-{in,of}-lhs). Oracle:
// node v26.2.0 (.expected) — bun 1.4.2 rejects the head at parse time
// ("Invalid assignment target"), the one place its transpiler skips
// Annex B.
var fCalled = false;
var valueOfCalled = false;
function f() {
  fCalled = true;
  return { valueOf() { valueOfCalled = true; return 1; } };
}
try { for (f() of [1]) {} } catch (e) { console.log("of", e instanceof ReferenceError, fCalled, valueOfCalled); }
fCalled = false;
try { for (f() in { a: 1 }) {} } catch (e) { console.log("in", e instanceof ReferenceError, fCalled, valueOfCalled); }
fCalled = false;
for (f() of []) {}
console.log("empty", fCalled);
