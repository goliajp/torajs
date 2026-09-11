// §16.1.7 — global code's `this` is the global object, so a property
// written through it is a property of `globalThis` and vice versa.
// bun treats a `.cts` entry as a CommonJS module whose top-level
// `this` is the exports object, so the oracle here is node's real
// script goal (`vm.runInThisContext`), pinned in `.expected`.
this.p = 1;
console.log(globalThis.p, this === globalThis, this.p);
globalThis.r = 2;
console.log(this.r);
this.p = 3;
console.log(globalThis.p);
