// v0.2 #2 Phase 2.0a: Date class substrate.
// new Date() / new Date(ms) / Date.now() / .getTime() / .valueOf() /
// .toISOString(). Component ctor, parse, get*/set* are Phase 2.0b.

const t = Date.now();
console.log(t > 1700000000000);
console.log(t < 2000000000000);

const d1 = new Date(0);
console.log(d1.getTime());
console.log(d1.toISOString());

const d2 = new Date(1577836800000);  // 2020-01-01T00:00:00.000Z
console.log(d2.getTime());
console.log(d2.toISOString());

const d3 = new Date(1700000000000);  // 2023-11-14T22:13:20.000Z
console.log(d3.toISOString());

const d4 = new Date(123456789);
console.log(d4.valueOf());
console.log(d4.toISOString());

// Pre-epoch
const d5 = new Date(-1);
console.log(d5.toISOString());

// Round-trip
const d6 = new Date(Date.now());
console.log(d6.getTime() > 1700000000000);
