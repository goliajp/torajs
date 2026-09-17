// §26.2.1 / §26.2.2 / §26.2.3 — the FinalizationRegistry constructor
// and its prototype object, before any cell is registered.
const fr = new FinalizationRegistry((h: any) => {
  console.log("cleanup", h);
});
console.log(typeof fr, Object.prototype.toString.call(fr));
console.log(typeof FinalizationRegistry, FinalizationRegistry.length, FinalizationRegistry.name);
console.log(Object.getPrototypeOf(fr) === FinalizationRegistry.prototype);
console.log(fr instanceof FinalizationRegistry, fr.constructor === FinalizationRegistry);
console.log(Object.keys(fr).length, Object.isExtensible(fr));
(fr as any).x = 1;
console.log((fr as any).x, Object.keys(fr));

try {
  new FinalizationRegistry(1 as any);
} catch (e: any) {
  console.log(e.constructor.name, e.message);
}
try {
  (FinalizationRegistry as any)(() => {});
} catch (e: any) {
  console.log(e.constructor.name);
}

const F: any = FinalizationRegistry;
const fr2 = new F(() => 0);
console.log(Object.getPrototypeOf(fr2) === F.prototype, Reflect.construct(F, [() => 0]) instanceof F);
try {
  Reflect.construct(F, [{}]);
} catch (e: any) {
  console.log(e.constructor.name, e.message);
}
console.log(Object.prototype.toString.call(FinalizationRegistry.prototype));
console.log(typeof globalThis.FinalizationRegistry, globalThis.FinalizationRegistry === F);
const d = Object.getOwnPropertyDescriptor(FinalizationRegistry.prototype, Symbol.toStringTag)!;
console.log(d.value, d.writable, d.enumerable, d.configurable);
const c = Object.getOwnPropertyDescriptor(FinalizationRegistry.prototype, "constructor")!;
console.log(c.value === F, c.writable, c.enumerable, c.configurable);
// §7.2.3 IsCallable — a Proxy over a function is callable.
const callableProxy = new Proxy(function () {}, {});
console.log(typeof new FinalizationRegistry(callableProxy as any));
try {
  new FinalizationRegistry(new Proxy({}, {}) as any);
} catch (e: any) {
  console.log(e.constructor.name);
}
