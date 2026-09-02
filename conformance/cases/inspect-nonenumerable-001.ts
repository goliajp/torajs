// console.log prints every own property, enumerable or not — bun's
// inspect walks the object's own keys without the enumerable filter
// (`Object.keys` / `JSON.stringify` / for-in keep excluding them).
// Plain objects, accessors, arrays, class instances, Symbol keys.
const s = Symbol("s");
const h: any = { k: 1 };
Object.defineProperty(h, s, { value: 2, enumerable: false });
Object.defineProperty(h, "hid", { value: 3, enumerable: false });
Object.defineProperty(h, Symbol("w"), { value: 4, enumerable: true, writable: false });
console.log(h);
console.log(Object.keys(h), Object.getOwnPropertySymbols(h).length, JSON.stringify(h));
for (const k in h) console.log("for-in", k);
const o: any = { k: 1 };
Object.defineProperty(o, "a", { value: 3 });
Object.defineProperty(o, "b", { value: 4, enumerable: false, writable: true, configurable: true });
o.c = 5;
Object.defineProperty(o, "c", { enumerable: false });
console.log(o);
const g: any = {};
Object.defineProperty(g, "a", { get() { return 1 }, enumerable: false });
Object.defineProperty(g, "b", { set(v: any) {}, enumerable: false });
console.log(g);
const arr: any = [1, 2];
Object.defineProperty(arr, "hid", { value: 9, enumerable: false });
console.log(arr);
class D { x = 1 }
const d: any = new D();
Object.defineProperty(d, "hid", { value: 3, enumerable: false });
d.e = 6;
Object.defineProperty(d, "e", { enumerable: false });
Object.defineProperty(d, s, { value: 7, enumerable: false });
console.log(d);
console.log(Object.keys(d), JSON.stringify(d));
const nested: any = { inner: {} };
Object.defineProperty(nested.inner, "deep", { value: [1, 2], enumerable: false });
console.log(nested);
// The keys bun leaves out: an own `constructor` (even enumerable), and
// a non-enumerable `__proto__`; an enumerable own `__proto__` prints.
console.log({ constructor: 1, a: 2 });
const oc: any = {}; oc.constructor = 2; oc.b = 3;
console.log(oc);
const pp: any = {}; Object.defineProperty(pp, "__proto__", { value: 5, enumerable: true }); pp.c = 6;
console.log(pp, Object.keys(pp));
const ac: any = [1]; ac.constructor = 9; ac.z = 8;
console.log(ac);
