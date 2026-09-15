// §10.5.5 / §10.5.6 / §10.5.11 — getOwnPropertyDescriptor,
// defineProperty and ownKeys trap results are checked against the
// target's committed properties.
function kind(f: () => unknown): string {
  try {
    const v = f();
    return "ok " + JSON.stringify(v);
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other " + String(e);
  }
}

const frozenProp: any = {};
Object.defineProperty(frozenProp, "k", { value: 1, writable: false, configurable: false, enumerable: true });

// getOwnPropertyDescriptor
console.log("gopd hide non-configurable", kind(() => Object.getOwnPropertyDescriptor(new Proxy(frozenProp, { getOwnPropertyDescriptor: () => undefined }), "k")));
const sealed: any = Object.preventExtensions({ a: 1 });
console.log("gopd hide on non-extensible", kind(() => Object.getOwnPropertyDescriptor(new Proxy(sealed, { getOwnPropertyDescriptor: () => undefined }), "a")));
console.log("gopd invent non-configurable", kind(() => Object.getOwnPropertyDescriptor(new Proxy({}, { getOwnPropertyDescriptor: () => ({ value: 1, configurable: false }) }), "x")));
console.log("gopd invalid descriptor", kind(() => Object.getOwnPropertyDescriptor(new Proxy({ x: 1 }, { getOwnPropertyDescriptor: () => ({ get: 1 }) }), "x")));
console.log("gopd change value", kind(() => Object.getOwnPropertyDescriptor(new Proxy(frozenProp, { getOwnPropertyDescriptor: () => ({ value: 2, writable: false, configurable: false, enumerable: true }) }), "k")));
const partial = { value: 5, configurable: true };
const got = Object.getOwnPropertyDescriptor(new Proxy({ p: 0 }, { getOwnPropertyDescriptor: () => partial }), "p");
console.log("gopd completes", JSON.stringify(got), JSON.stringify(partial));

// defineProperty
console.log("define on non-extensible", kind(() => Object.defineProperty(new Proxy(Object.preventExtensions({}), { defineProperty: () => true }), "n", { value: 1 })));
console.log("define non-configurable missing", kind(() => Object.defineProperty(new Proxy({}, { defineProperty: () => true }), "n", { value: 1, configurable: false })));
console.log("define incompatible", kind(() => Object.defineProperty(new Proxy(frozenProp, { defineProperty: () => true }), "k", { value: 9 })));
const wnc: any = {};
Object.defineProperty(wnc, "w", { value: 1, writable: true, configurable: false });
console.log("define writable to non-writable", kind(() => Object.defineProperty(new Proxy(wnc, { defineProperty: () => true }), "w", { value: 1, writable: false })));
console.log("define accepted", kind(() => Reflect.defineProperty(new Proxy({}, { defineProperty: () => true }), "n", { value: 1 })));
console.log("define refused", kind(() => Reflect.defineProperty(new Proxy({}, { defineProperty: () => false }), "n", { value: 1 })));

// ownKeys
console.log("keys wrong type", kind(() => Object.keys(new Proxy({}, { ownKeys: () => [1] }))));
console.log("keys duplicate", kind(() => Object.keys(new Proxy({}, { ownKeys: () => ["a", "a"] }))));
console.log("keys missing non-configurable", kind(() => Object.keys(new Proxy(frozenProp, { ownKeys: () => [] }))));
console.log("keys new on non-extensible", kind(() => Reflect.ownKeys(new Proxy(Object.preventExtensions({ a: 1 }), { ownKeys: () => ["a", "b"] }))));
console.log("keys missing on non-extensible", kind(() => Reflect.ownKeys(new Proxy(Object.preventExtensions({ a: 1, b: 2 }), { ownKeys: () => ["a"] }))));
const s1 = Symbol("s1");
const withSym: any = { x: 1 };
withSym[s1] = 2;
const symP = new Proxy(withSym, { ownKeys: (t: any) => Reflect.ownKeys(t) });
console.log("symbols through trap", Object.getOwnPropertySymbols(symP).length, Object.getOwnPropertySymbols(symP)[0] === s1);
console.log("ownKeys through trap", Reflect.ownKeys(symP).length, String(Reflect.ownKeys(symP)[0]));
console.log("forward to proxy target", Reflect.ownKeys(new Proxy(new Proxy(withSym, {}), {})).length);
console.log("keys ok", JSON.stringify(Object.keys(new Proxy({ a: 1, b: 2 }, { ownKeys: () => ["b", "a"] }))));
