// §10.1.6.3 over one storage — a configurable global binding (an
// implicit global's property is configurable, §9.1.1.4.6) accepts a
// `defineProperty` that changes `configurable` / `enumerable`; only a
// `writable: false` is refused (the compiled bare writes go straight
// into the slot and would not see it — a registered tr limitation).
// Before RFC 20260912-global-var-cell's redefine relaxation, ANY
// attribute change threw "not yet supported".
//
// Oracle is node's script goal (vm.runInThisContext); bun runs a
// `.cts` entry as CommonJS, where these are module-local.
x = 1;
var d0 = Object.getOwnPropertyDescriptor(this, "x");
console.log(d0.configurable, d0.enumerable, d0.writable, x);
Object.defineProperty(this, "x", { configurable: false, value: 5 });
var d1 = Object.getOwnPropertyDescriptor(this, "x");
console.log(d1.configurable, d1.enumerable, d1.writable, x, globalThis.x);
y = 2;
Object.defineProperty(this, "y", { enumerable: false });
var d2 = Object.getOwnPropertyDescriptor(this, "y");
console.log(d2.enumerable, d2.configurable, y, Object.keys(this).includes("y"));
