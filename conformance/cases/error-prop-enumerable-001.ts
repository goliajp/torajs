// ES §20.5.6.1.1 — an error instance's `message` is
// { W:1, E:0, C:1 }, and the `stack` header line carries the same
// attributes. Neither is an ordinary enumerable class field, so the
// enumerable-only surfaces must skip both while the own-ness
// surfaces keep them.
const e = new Error("m");
console.log("pie message:", e.propertyIsEnumerable("message"));
console.log("pie stack:", e.propertyIsEnumerable("stack"));
console.log("own message:", e.hasOwnProperty("message"));
console.log("own stack:", e.hasOwnProperty("stack"));

const dm = Object.getOwnPropertyDescriptor(e, "message");
const ds = Object.getOwnPropertyDescriptor(e, "stack");
console.log("desc message enum:", dm ? dm.enumerable : "missing");
console.log("desc stack enum:", ds ? ds.enumerable : "missing");
console.log("desc message value:", dm ? dm.value : "missing");

// A runtime key reaches the same verdict as the literal one — the
// two take different lowering paths (compile-time fold vs str_eq
// chain), and they must not disagree.
const k1 = "mess" + "age";
const k2 = "sta" + "ck";
console.log("pie runtime message:", e.propertyIsEnumerable(k1));
console.log("pie runtime stack:", e.propertyIsEnumerable(k2));
console.log("own runtime stack:", e.hasOwnProperty(k2));

// §20.5.1.1 — a no-arg construction defines no own `message` at all,
// while `stack` is written by every construction.
const e0 = new Error();
console.log("noarg own message:", e0.hasOwnProperty("message"));
console.log("noarg own stack:", e0.hasOwnProperty("stack"));
console.log("noarg pie stack:", e0.propertyIsEnumerable("stack"));

// Subclasses inherit the attributes, both the injected NativeError
// family and a user-written one.
const t = new TypeError("t");
console.log("typeerror pie stack:", t.propertyIsEnumerable("stack"));
console.log("typeerror own message:", t.hasOwnProperty("message"));
class W extends Error {
  constructor(m: string) {
    super(m);
    this.name = "W";
  }
}
const w = new W("w");
console.log("user pie stack:", w.propertyIsEnumerable("stack"));
console.log("user pie message:", w.propertyIsEnumerable("message"));
console.log("user own stack:", w.hasOwnProperty("stack"));

// Negative control — the rule keys on the error header bit, not on
// the field spelling: an ordinary class carrying the same two names
// keeps the all-true attributes.
class Box {
  message: string;
  stack: string;
  constructor(m: string) {
    this.message = m;
    this.stack = "s";
  }
}
const b = new Box("b");
console.log("box pie message:", b.propertyIsEnumerable("message"));
console.log("box pie stack:", b.propertyIsEnumerable("stack"));
console.log("box keys:", JSON.stringify(Object.keys(b)));
console.log("box json:", JSON.stringify(b));
