// set Iterator.prototype.constructor on a receiver that already owns
// `constructor` performs Set on that own property (§27.1.2.1 step 5),
// whether the receiver is a typed object literal or a dynamic object
const s = {};
const d = Object.getOwnPropertyDescriptor(Iterator.prototype, "constructor") as any;

const o = { constructor: s + "a" };
d.set.call(o, s);
console.log(typeof o.constructor, o.constructor === s);

const p = { constructor: "x", n: 1 };
d.set.call(p, "y");
console.log(p.constructor, p.n, Object.keys(p).join(","));

const q: any = { constructor: 1 };
d.set.call(q, 2);
console.log(q.constructor);

const r = { other: 1 };
d.set.call(r, s);
console.log((r as any).constructor === s, Object.keys(r).join(","));

console.log(Iterator.prototype.constructor === Iterator);
