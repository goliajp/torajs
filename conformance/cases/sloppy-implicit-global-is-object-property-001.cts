// §9.1.1.4.6 — an implicit global IS a property of the global object,
// with the default attributes. tr used to answer this by synthesizing
// a hoisted `var`, which is a binding: invisible on `globalThis`, and
// non-configurable where §9.1.1.4.6 creates a configurable property.
//
// Oracle is node's real script goal (vm.runInThisContext), pinned in
// `.expected` — bun runs a `.cts` entry as CommonJS.
zz2 = 7;
console.log(zz2, globalThis.zz2, "zz2" in globalThis);
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(globalThis, "zz2")));
