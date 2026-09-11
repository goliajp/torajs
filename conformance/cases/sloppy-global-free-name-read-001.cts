// §9.1.1.4 GetBindingValue on the object half of the global
// environment record: a bare name with no static lane reads the global
// object at run time, and only throws ReferenceError when the object
// does not carry it either.
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
console.log(typeof nope9);
this.p = 5;
console.log(p);
