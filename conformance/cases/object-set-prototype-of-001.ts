// §20.1.2.21 Object.setPrototypeOf + Annex B §B.2.2.1 `__proto__`
// setter (RFC 20260717-user-proto-chain knife 3) — re-parent takes
// effect on reads, null unlinks, the cycle refusal throws, and the
// setter face silently ignores invalid values.

const a: any = { kind: "a" };
const b: any = { kind: "b", extra: 1 };
const o: any = Object.create(a);
console.log(o.kind); // a

// re-parent
Object.setPrototypeOf(o, b);
console.log(Object.getPrototypeOf(o) === b); // true
console.log(o.kind); // b
console.log(o.extra); // 1

// answers the receiver
console.log(Object.setPrototypeOf(o, b) === o); // true

// null unlinks
Object.setPrototypeOf(o, null);
console.log(Object.getPrototypeOf(o)); // null
console.log(o.kind); // undefined

// re-link from null-proto shape
Object.setPrototypeOf(o, a);
console.log(o.kind); // a

// cycle refusal
const p: any = Object.create(o);
let caught = "";
try {
  Object.setPrototypeOf(o, p);
} catch (e: any) {
  caught = "cycle";
}
console.log(caught); // cycle

// primitive receiver passes through
console.log(Object.setPrototypeOf(7 as any, null)); // 7

// invalid proto throws
caught = "";
try {
  Object.setPrototypeOf(o, 42 as any);
} catch (e: any) {
  caught = "invalid";
}
console.log(caught); // invalid

// __proto__ setter face
const s: any = Object.create(a);
s.__proto__ = b;
console.log(s.kind); // b
console.log(Object.getPrototypeOf(s) === b); // true

// setter silently ignores a primitive value
s.__proto__ = 5;
console.log(Object.getPrototypeOf(s) === b); // true

// setter to null
s.__proto__ = null;
console.log(Object.getPrototypeOf(s)); // null
console.log("done");
