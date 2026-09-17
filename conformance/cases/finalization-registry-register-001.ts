// §26.2.3.2 register / §26.2.3.3 unregister — argument validation,
// return values, and the token bookkeeping that needs no collection.
// The callback stays silent: tr collects the temporaries below at
// once and runs it at exit, bun (without a forced gc) never does.
const fr = new FinalizationRegistry((h: any) => {});
const o = {};
const probes: Array<() => unknown> = [
  () => fr.register(1 as any, 1),
  () => fr.register(o, o),
  () => fr.register(o, 1, 1 as any),
  () => fr.unregister(1 as any),
  () => fr.unregister(undefined as any),
  () => fr.register(Symbol.for("x") as any, 1),
  () => fr.unregister(Symbol.for("y") as any),
];
for (const p of probes) {
  try {
    console.log(p());
  } catch (e: any) {
    console.log(e.constructor.name, e.message);
  }
}

const tok = {};
const t1 = {};
const t2 = {};
console.log(fr.register(t1, "a", tok), fr.register(t2, "b", tok), fr.register(t1, "c"));
console.log(fr.unregister(tok), fr.unregister(tok), fr.unregister(t1));
console.log(fr.register(Symbol("s") as any, 1), fr.register(fr, 1, fr), fr.unregister(fr), fr.unregister(fr));
console.log(fr.register(t1, t2, t1), fr.unregister(t1));

const register = FinalizationRegistry.prototype.register;
const unregister = FinalizationRegistry.prototype.unregister;
console.log(register.length, register.name, unregister.length, unregister.name);
console.log(register.call(fr, {}), unregister.call(fr, {}));
for (const r of [{}, WeakRef.prototype, new WeakMap(), 1]) {
  try {
    (register as any).call(r, {}, 1);
    console.log("no throw");
  } catch (e: any) {
    console.log(e.constructor.name);
  }
}
console.log(Object.getOwnPropertyNames(FinalizationRegistry.prototype).sort().join());
