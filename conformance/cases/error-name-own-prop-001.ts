// ES §20.5.3.2 / §20.5.6.3 — `name` is an own property of
// `Error.prototype` (and of each NativeError prototype), never of the
// instance. An instance owns one only where user code assigned it,
// and that assignment is an ordinary enumerable data property.
const e = new Error("m");
console.log("keys:", JSON.stringify(Object.keys(e)));
console.log("json:", JSON.stringify(e));
console.log("own name:", e.hasOwnProperty("name"));
console.log("name:", e.name);

const t = new TypeError("t");
console.log("sub keys:", JSON.stringify(Object.keys(t)));
console.log("sub own name:", t.hasOwnProperty("name"));
console.log("sub name:", t.name);
const r = new RangeError("r");
console.log("range name:", r.name, "keys:", JSON.stringify(Object.keys(r)));

// A user subclass that never assigns inherits through its own
// prototype up to Error.prototype.
class Bare extends Error {}
const b = new Bare("b");
console.log("bare keys:", JSON.stringify(Object.keys(b)));
console.log("bare own name:", b.hasOwnProperty("name"));
console.log("bare name:", b.name);

// A user subclass that DOES assign owns it — and then it enumerates.
class W extends Error {
  constructor(m: string) {
    super(m);
    this.name = "W";
  }
}
const w = new W("w");
console.log("user keys:", JSON.stringify(Object.keys(w)));
console.log("user json:", JSON.stringify(w));
console.log("user own name:", w.hasOwnProperty("name"));
console.log("user pie name:", w.propertyIsEnumerable("name"));
console.log("user name:", w.name);

// Descriptors: absent on a plain error, ordinary all-true on the
// assigned one.
const de = Object.getOwnPropertyDescriptor(e, "name");
console.log("desc plain:", de ? "present" : "undefined");
const dw = Object.getOwnPropertyDescriptor(w, "name");
console.log("desc user enum:", dw ? dw.enumerable : "missing");
console.log("desc user value:", dw ? dw.value : "missing");

// Runtime keys take the str_eq chain rather than the compile-time
// fold; the two must agree.
const k = "na" + "me";
console.log("runtime own plain:", e.hasOwnProperty(k));
console.log("runtime own user:", w.hasOwnProperty(k));
console.log("runtime pie user:", w.propertyIsEnumerable(k));

// The any lane reads the same state through different substrate.
const ae: any = e;
const aw: any = w;
console.log("any name plain:", ae.name);
console.log("any name user:", aw.name);
console.log("any vals plain:", JSON.stringify(Object.values(ae)));
console.log("any ents user:", JSON.stringify(Object.entries(aw)));

const seen: string[] = [];
for (const key in e) { seen.push(key); }
console.log("forin plain:", JSON.stringify(seen));
const seen2: string[] = [];
for (const key in w) { seen2.push(key); }
console.log("forin user:", JSON.stringify(seen2));

// String(err) and .stack read the resolved name, not the slot.
console.log("string plain:", String(e));
console.log("string sub:", String(t));
console.log("string user:", String(w));
console.log("string bare:", String(b));

// Negative control — an ordinary class named field stays an ordinary
// own enumerable property.
class Tag {
  name: string;
  constructor(n: string) {
    this.name = n;
  }
}
const g = new Tag("g");
console.log("tag keys:", JSON.stringify(Object.keys(g)));
console.log("tag own name:", g.hasOwnProperty("name"));
console.log("tag json:", JSON.stringify(g));
