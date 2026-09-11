// §9.1.1.4 GetBindingValue with `S` = false — a bare name that
// resolves to no binding consults the OBJECT half of the global
// environment record before it throws. Global code's `this` is that
// object, so `this.p = 1` and the bare `p` name the same property.
//
// Oracle is node's real script goal (vm.runInThisContext), pinned in
// `.expected` — bun runs a `.cts` entry as CommonJS, where top-level
// `this` is the exports object rather than the global object.
this.p = 1;
console.log(p);
function f(): any {
  return p;
}
console.log(f());
globalThis.r = 2;
console.log(r);
try {
  nope9;
} catch (e) {
  console.log((e as any).constructor.name, (e as any).message);
}
this.p = 5;
console.log(p);
