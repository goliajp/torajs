// RFC 20260720-symbol-any-call-boundary — Symbol.for / Symbol.keyFor
// direct calls with an Any-typed argument route to the any-lane
// kernels (ToString coercion / §20.4.2.6 brand check) and answer
// NaN-box values (unregistered → undefined, not null).
const s = Symbol.for("app.token");
const x: any = s;
console.log(Symbol.keyFor(x));
const u: any = Symbol("local");
console.log(Symbol.keyFor(u));
const n: any = 42;
try { Symbol.keyFor(n); } catch (e) { console.log("caught"); }
const k: any = "reg.key";
const s2 = Symbol.for(k);
console.log(typeof s2);
console.log(Symbol.keyFor(s2));
const num: any = 7;
const s3 = Symbol.for(num);
console.log(Symbol.keyFor(s3));
