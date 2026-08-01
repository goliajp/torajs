// §15.8.4 EvaluateAsyncFunctionBody steps 2-3: an abrupt completion
// from parameter default instantiation rejects the promise instead
// of throwing synchronously
function boom(): any { throw new Error("boom"); }
async function f1(x = boom()) { return 1; }
f1().then(
  (v) => console.log("resolved", v),
  (e) => console.log("rejected", e.message)
);
async function f2(_ = (function() { throw new Error("iife"); }())) { return 2; }
f2().then(
  (v) => console.log("resolved2", v),
  (e) => console.log("rejected2", e.message)
);
// closed fn-literal default on a sync fn still materializes in the
// callee scope
function s(cb = () => 42) { return cb(); }
console.log("sync", s());
async function ok(a: number, b = a + 1) { return a + b; }
ok(1).then((v) => console.log("ok", v));

// body must not be evaluated when instantiation throws
let callCount = 0;
async function f3(_ = (function() { throw new Error("abrupt"); }())) {
  callCount = callCount + 1;
}
f3().then(
  () => console.log("bad fulfill"),
  (e) => console.log("rejected3", e.message, "count", callCount)
);
// async class methods (instance + static) and async arrows reject too
class C {
  async m(x = (function(): any { throw new Error("meth"); }())) { return 1; }
  static async sm(x = (function(): any { throw new Error("smeth"); }())) { return 2; }
}
new C().m().then(
  () => console.log("bad m"),
  (e) => console.log("m rejected", e.message)
);
C.sm().then(
  () => console.log("bad sm"),
  (e) => console.log("sm rejected", e.message)
);
const aa = async (x = (function(): any { throw new Error("arrow"); }())) => 3;
aa().then(
  () => console.log("bad aa"),
  (e) => console.log("aa rejected", e.message)
);
