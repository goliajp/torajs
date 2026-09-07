// `Object.defineProperty` / `Reflect.defineProperty` on a receiver
// whose ordinary own face is a property bag. Three copies of the same
// arm — the literal fast path, the objlit-runtime road, and the
// Reflect flavor — each answered for Any / Arr / Closure / struct and
// then bailed: a silent no-op in the first two, `false` in the third.
// The `any` spelling of the identical receiver defined all along.

const m = new Map<string, number>();
m.set("k", 1);
console.log(Reflect.defineProperty(m as any, "tag", { value: 5, enumerable: true, configurable: true, writable: true }));
console.log((m as any).tag, m.get("k"), m.size);
Object.defineProperty(m as any, "two", { value: 6, enumerable: true, configurable: true, writable: true });
console.log((m as any).two, JSON.stringify(Object.keys(m as any)));

const s = new Set<number>();
Object.defineProperty(s as any, "q", { value: 7, enumerable: true, configurable: true, writable: true });
console.log((s as any).q, "q" in s, Object.hasOwn(s as any, "q"));

const d = new Date(0);
Object.defineProperty(d as any, "z", { value: 8, enumerable: true, configurable: true, writable: true });
console.log((d as any).z, d.getTime());

const re = /a/g;
Object.defineProperty(re as any, "z", { value: 9, enumerable: true, configurable: true, writable: true });
console.log((re as any).z, re.source);

const wm = new WeakMap<object, number>();
Object.defineProperty(wm as any, "z", { value: 10, enumerable: true, configurable: true, writable: true });
console.log((wm as any).z);

// A non-enumerable define is still invisible to the enumeration face.
const m2 = new Map<string, number>();
Object.defineProperty(m2 as any, "hidden", { value: 1, configurable: true });
console.log((m2 as any).hidden, JSON.stringify(Object.keys(m2 as any)));
console.log(Object.getOwnPropertyDescriptor(m2 as any, "hidden") !== undefined);

// The receivers that already worked, unchanged.
const o = { a: 1 };
console.log(Reflect.defineProperty(o as any, "b", { value: 2, enumerable: true, configurable: true, writable: true }), (o as any).b);
const arr = [1, 2];
console.log(Reflect.defineProperty(arr as any, "z", { value: 3, enumerable: true, configurable: true, writable: true }), (arr as any).z, arr.length);
