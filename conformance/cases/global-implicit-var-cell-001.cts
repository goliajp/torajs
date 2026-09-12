// An assignment to an undeclared name in sloppy code creates the
// global object's property AT THE WRITE (§9.1.1.4.6), not at program
// start: before it the name is absent from the object, after a
// `delete` the next write creates it again. RFC 20260912-global-var-cell.
//
// Oracle is node's real script goal (vm.runInThisContext), pinned in
// `.expected` — bun runs a `.cts` entry as CommonJS.
console.log("x" in globalThis, typeof x);
x = 1;
console.log("x" in globalThis, globalThis.x, delete globalThis.x, "x" in globalThis);
x = 2;
console.log("x" in globalThis, this.x, Object.getOwnPropertyDescriptor(this, "x").configurable);
function f() { y = 3; }
console.log("y" in globalThis);
f();
console.log("y" in globalThis, globalThis.y);
