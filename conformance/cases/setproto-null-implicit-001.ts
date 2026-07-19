// §10.1.2.1 step 3 — SameValue(V, current) compares against the TRUE
// [[Prototype]]: an absent-entry implicit chain's current is
// %Object.prototype%, not null, so setPrototypeOf(o, null) over an
// implicit-chain receiver is a real transition (mark the null-proto
// bit), while set.call(Object.prototype, null) stays a silent
// same-value no-op per §10.4.7.1.
const o: any = { a: 1 };
Object.setPrototypeOf(o, null);
console.log(Object.getPrototypeOf(o));
const s: any = Object.setPrototypeOf;
const p: any = { b: 2 };
s(p, null);
console.log(Object.getPrototypeOf(p));
const a: any = { x: 1 };
Object.setPrototypeOf(a, Object.prototype);
console.log(Object.getPrototypeOf(a) === Object.prototype);
Object.setPrototypeOf(Object.prototype, null);
console.log("op-silent-ok");
const b: any = { y: 2 };
Object.setPrototypeOf(b, null);
const base: any = { g: 9 };
Object.setPrototypeOf(b, base);
console.log(Object.getPrototypeOf(b) === base, b.g);
const c: any = { z: 3 };
Object.freeze(c);
try { Object.setPrototypeOf(c, null); } catch (e) { console.log("frozen-caught"); }
const d: any = {};
const e2: any = {};
Object.setPrototypeOf(d, e2);
try { Object.setPrototypeOf(e2, d); } catch (e) { console.log("cycle-caught"); }
