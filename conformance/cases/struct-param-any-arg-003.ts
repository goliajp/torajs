// §13.3.2.1 / §10.1.8.1 / §10.1.9 — an object literal of another
// shape handed to a struct-typed parameter through `any`: a getter
// runs once per read (two reads, two calls), an inherited property is
// found on the prototype, and a write lands on the original object.
type P = { x: number; y: number };
function one(p: P) { return p.x; }
function twice(p: P) { return p.x + p.x; }
function w(p: P) { p.x = 99; return p.y; }
const g1: any = one;
const g2: any = twice;
const gw: any = w;
let n = 0;
const acc = { get x() { n++; return 7; }, y: 0 };
console.log("getter once ->", g1(acc), "n", n);
console.log("getter twice ->", g2(acc), "n", n);
console.log("proto ->", g1(Object.create({ x: 1, y: 2 })));
const lit: any = { x: 1, y: 2 };
console.log("write ->", gw(lit), "then", lit.x);
console.log("typed", one({ x: 3, y: 4 }), twice({ x: 3, y: 4 }));
