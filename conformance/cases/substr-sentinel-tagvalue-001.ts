// Substr undefined sentinel crossing the (tag, value) boundary
// (rotation 185): Map.set / dynobj member-assign of an OOB string
// index read decode to a real undefined.
const s = "hello";
const e = s[99];
const m = new Map();
m.set("k", e);
const g = m.get("k");
console.log(g);
console.log(typeof g);
console.log(g === undefined);
// in-range view keeps its string face through the same route
const h = s[1];
m.set("h", h);
const g2 = m.get("h");
console.log(g2, typeof g2, g2 === undefined);
// dynobj member-assign lane
const o: any = {};
o.miss = s[42];
o.hit = s[0];
console.log(o.miss, typeof o.miss);
console.log(o.hit, typeof o.hit);
console.log(o);
// str-slot sentinel through the same member-assign pack (missed
// regex capture)
const mm = "x".match(/x(y)?/);
o.cap = mm[1];
console.log(o.cap, typeof o.cap, o.cap === undefined);
const nn = "xy".match(/x(y)?/);
o.cap2 = nn[1];
console.log(o.cap2, typeof o.cap2);
