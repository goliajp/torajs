// S2.35b — a top-level `var obj = {}` (empty object literal, no
// define/expando trigger) promotes as an Any global, so named-fn and
// class-method bodies can read it. Previously the binding fell into
// a gap (not degraded, not __inlobj, not shaped) and every fn read
// died with "unknown identifier".

// plain named fn reads it (identity compare)
var obj = {};
function f(a: any) {
  console.log("fn", a === obj);
}
f(obj);

// class method reads it — with default params, the test262
// dflt-params shape
var count = 0;
class C {
  method(aFalse = count += 1, aObj = count += 1) {
    console.log("meth", aFalse === false, aObj === obj);
  }
}
C.prototype.method(false, obj);
console.log("count", count);

// expando write from a named fn lands on the dynobj cell
var bag = {};
function put() {
  (bag as any).k = 7;
}
put();
console.log("expando", (bag as any).k);

// main-side reassignment keeps the shared home
var slot = {};
function show(tag: string) {
  console.log(tag, slot === obj, (slot as any).v);
}
show("pre");
(slot as any).v = 1;
show("post");

// main-only empty literal stays main-local and keeps working
var localOnly = {};
console.log("local", typeof localOnly);
