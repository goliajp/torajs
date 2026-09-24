// §23.1.3.28 slice copies only PRESENT indices: a hole of the receiver
// (a literal elision, `new Array(n)`, a delete, an accessor-free absent
// index) stays absent in the product; an index Array.prototype supplies
// is present and copied as an own property.
const a: any = [0, , 2, , 4];
const s = a.slice(1, 4);
console.log(s.length, 0 in s, 1 in s, 2 in s, Object.keys(s).join(), JSON.stringify(s));
const n: any = new Array(3);
n[1] = "m";
const ns = n.slice();
console.log(ns.length, 0 in ns, 1 in ns, Object.keys(ns).join());
const d: any = [1, 2, 3];
delete d[0];
console.log(Object.keys(d.slice()).join(), d.slice(0, 2).length);
const t = [1, 2, 3].slice(1);
console.log(JSON.stringify(t), Object.keys(t).join());
Array.prototype[1] = "P";
const p = a.slice();
console.log(p.hasOwnProperty(1), p[1], p.hasOwnProperty(3));
delete Array.prototype[1];
console.log(JSON.stringify(Array.from(a)), JSON.stringify(a.toReversed()), Object.keys(a.toReversed()).join());
const b: any[] = [0, , 2];
const bs = b.slice();
console.log(bs.length, 1 in bs, Object.keys(bs).join());
