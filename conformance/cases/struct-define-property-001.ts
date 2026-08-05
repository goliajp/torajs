// `Object.defineProperty` on a class instance did NOTHING — no entry,
// no throw, and the property still absent afterwards. Every own-
// property READ already consulted the instance's `+24` expando dict;
// defining into it was the one direction missing.
class Box {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}

// A literal descriptor.
const a = new Box(1);
Object.defineProperty(a, "hid", { value: 5, enumerable: false });
const aa: any = a;
console.log("lit read:", aa.hid);
console.log("lit has:", a.hasOwnProperty("hid"));
console.log("lit pie:", a.propertyIsEnumerable("hid"));
console.log("lit desc:", JSON.stringify(Object.getOwnPropertyDescriptor(a, "hid")));
console.log("lit keys:", JSON.stringify(Object.keys(a)));

// A descriptor held in a variable takes a different lowering road,
// and the two have to define the same entry — the read side cannot
// tell which of them wrote it.
const b = new Box(1);
const d: any = { value: 7, writable: true, enumerable: true, configurable: true };
Object.defineProperty(b, "rt", d);
console.log("rt desc:", JSON.stringify(Object.getOwnPropertyDescriptor(b, "rt")));
console.log("rt keys:", JSON.stringify(Object.keys(b)));

// An `any`-typed receiver holding the same instance is the third
// road, dispatched on the cell's tag at runtime rather than statically.
const c: any = new Box(1);
Object.defineProperty(c, "viaAny", { value: 9, enumerable: true });
console.log("any read:", c.viaAny);
console.log("any keys:", JSON.stringify(Object.keys(c)));

// Reflect shares the kernel.
const e = new Box(1);
Reflect.defineProperty(e, "refl", { value: 3, enumerable: true });
console.log("reflect:", JSON.stringify(Object.getOwnPropertyDescriptor(e, "refl")));

// The defaults an omitted attribute takes are the spec's, not the
// ones a declared field carries.
const f = new Box(1);
Object.defineProperty(f, "bare", { value: 1 });
console.log("bare:", JSON.stringify(Object.getOwnPropertyDescriptor(f, "bare")));
console.log("bare keys:", JSON.stringify(Object.keys(f)));

// A declared field still answers off the layout.
console.log("field:", JSON.stringify(Object.getOwnPropertyDescriptor(a, "v")));

// §10.1.6.3 step 2 — the extensibility flag lives on the instance
// header, not on the dict the define descends into, so it has to be
// read before descending: a NEW key is refused, an existing one still
// updates.
const g = new Box(1);
Object.preventExtensions(g);
try {
  Object.defineProperty(g, "nope", { value: 1 });
  console.log("nonext: no throw");
} catch (err: any) {
  console.log("nonext threw:", err instanceof TypeError);
}
console.log("nonext desc:", Object.getOwnPropertyDescriptor(g, "nope"));

const h = new Box(1);
Object.defineProperty(h, "k", { value: 1, configurable: true });
Object.preventExtensions(h);
Object.defineProperty(h, "k", { value: 2 });
console.log("nonext update:", JSON.stringify(Object.getOwnPropertyDescriptor(h, "k")));
