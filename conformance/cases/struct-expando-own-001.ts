// A class instance can carry expando entries beyond its declared
// fields — `err.cause` is one the Error constructor itself installs.
// Those are own properties, so hasOwnProperty / propertyIsEnumerable
// must see them; a fold over the compile-time field list cannot.
const e = new Error("m", { cause: 42 });
console.log("ctor own:", e.hasOwnProperty("cause"));
console.log("ctor pie:", e.propertyIsEnumerable("cause"));

// A user's own assignment is enumerable — the two shapes differ and
// both have to be reported from the live entry, not from a layout.
const w = new Error("m");
const wa: any = w;
wa.cause = 7;
console.log("user own:", w.hasOwnProperty("cause"));
console.log("user pie:", w.propertyIsEnumerable("cause"));

// No cause at all.
const n = new Error("m");
console.log("none own:", n.hasOwnProperty("cause"));
console.log("none pie:", n.propertyIsEnumerable("cause"));

// The literal key and a runtime key take different lowering paths
// (compile-time fold vs str_eq chain) and must agree.
const k = "cau" + "se";
console.log("ctor own rt:", e.hasOwnProperty(k));
console.log("ctor pie rt:", e.propertyIsEnumerable(k));
console.log("user own rt:", w.hasOwnProperty(k));
console.log("user pie rt:", w.propertyIsEnumerable(k));
console.log("none own rt:", n.hasOwnProperty(k));

// An ordinary class grows expandos the same way.
class Box {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}
const b = new Box(1);
const ba: any = b;
ba.extra = "x";
console.log("box field own:", b.hasOwnProperty("v"));
console.log("box field pie:", b.propertyIsEnumerable("v"));
console.log("box expando own:", b.hasOwnProperty("extra"));
console.log("box expando pie:", b.propertyIsEnumerable("extra"));
console.log("box expando own rt:", b.hasOwnProperty("ext" + "ra"));
console.log("box absent own:", b.hasOwnProperty("nope"));
console.log("box absent own rt:", b.hasOwnProperty("no" + "pe"));
console.log("box keys:", JSON.stringify(Object.keys(b)));
