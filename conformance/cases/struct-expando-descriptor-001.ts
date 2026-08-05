// `Object.getOwnPropertyDescriptor` on a class instance folds against
// the compile-time field list, so every expando entry — an own
// property written at runtime — answered `undefined` while
// `Object.keys` on the SAME object listed it.
class Box {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}
const b = new Box(1);
const ba: any = b;
ba.extra = "x";

// A layout field still answers off the layout.
console.log("field:", JSON.stringify(Object.getOwnPropertyDescriptor(b, "v")));
// The expando answers off the live entry.
console.log("expando:", JSON.stringify(Object.getOwnPropertyDescriptor(b, "extra")));
// A genuine miss is still `undefined`.
console.log("absent:", Object.getOwnPropertyDescriptor(b, "nope"));
// Both spellings share the kernel.
console.log("reflect:", JSON.stringify(Reflect.getOwnPropertyDescriptor(b, "extra")));

// The descriptor reads the entry's OWN W/E/C flags rather than
// synthesizing the all-true attributes a data field carries: the
// Error ctor installs `cause` as `[[Enumerable]]: false` (§20.5.8.1),
// which no layout field can be.
const e = new Error("m", { cause: 42 });
console.log("ctor cause:", JSON.stringify(Object.getOwnPropertyDescriptor(e, "cause")));
// A user's own assignment is an ordinary enumerable data property.
const w = new Error("m");
const wa: any = w;
wa.cause = 7;
console.log("user cause:", JSON.stringify(Object.getOwnPropertyDescriptor(w, "cause")));

// A symbol-keyed expando has nowhere but the dict to live — the
// layout metadata is name-keyed by construction.
const s = Symbol("k");
ba[s] = 9;
const sd = Object.getOwnPropertyDescriptor(b, s);
console.log("symbol:", JSON.stringify(sd));

// The object agrees with itself across every own-property surface.
console.log("has:", b.hasOwnProperty("extra"));
console.log("pie:", b.propertyIsEnumerable("extra"));
console.log("keys:", JSON.stringify(Object.keys(b)));
