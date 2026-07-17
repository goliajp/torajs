// RFC 20260718-builtin-error-ctor-first-class knife 4 —
// Error.prototype.toString own function entry (§20.5.3.4) + the
// optional-message ctor face (§20.5.1.1, `new Error()` is legal).
const d = Object.getOwnPropertyDescriptor(Error.prototype, "toString");
console.log(typeof Error.prototype.toString);
console.log(d ? [typeof d.value, d.writable, d.enumerable, d.configurable].join(",") : "no-desc");
console.log(Error.prototype.toString.name, Error.prototype.toString.length);
// subclass prototypes inherit — no own entry, same identity
console.log(Object.getOwnPropertyDescriptor(RangeError.prototype, "toString") === undefined);
console.log(RangeError.prototype.toString === Error.prototype.toString);
// behavior stays consistent across every route
const e = new RangeError("boom");
console.log(e.toString());
console.log(Error.prototype.toString.call(e));
const anyE: any = new TypeError("x");
console.log(anyE.toString());
// optional message: zero-arg construction across the family
console.log(new Error().toString());
console.log(new Error().message === "");
console.log(new RangeError().toString());
console.log(new SyntaxError().name);
