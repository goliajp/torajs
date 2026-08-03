// r290 (box_to_any FnSig sweep cluster) — a top-FnDecl name used as
// a VALUE in positions with no annotation to key off: an untyped
// array-literal element, and an argument to an indirect callee.
// Both wrap through the closure forwarder so the any-boxing sites
// receive a closure cell instead of a raw fn address.
function a() {
  return 1;
}
function b() {
  return 2;
}
[a, b].forEach(function (f: any) {
  console.log(f());
});

// A typed fn-array consumer keeps working through the same wrap.
const fns = [a, b];
for (const f of fns) {
  console.log(f());
}

// Indirect callee: the binding is not a top-FnDecl name, so its
// argv packs any-boxed slots — the fn-name argument must arrive as
// a callable cell (§20.2 the callee here throws on an undefined
// this, which is exactly the assertion shape of the test262
// methods-called-as-functions family).
function callback() {
  return 3;
}
var every: any = Array.prototype.every;
try {
  every(callback);
} catch (e: any) {
  console.log("caught", e.constructor.name);
}

// The argument survives the trip as a callable.
const keep: any = (g: any) => g();
console.log(keep(callback));
