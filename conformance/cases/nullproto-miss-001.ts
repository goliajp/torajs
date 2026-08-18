// sec 10.1.8.1 OrdinaryGet step 2 — an explicit null [[Prototype]]
// cuts the chain: a miss answers undefined, no builtin surface.
const a = Object.create(null) as any;
a.x = 1;
console.log(a.x, a.toString === undefined, a.valueOf === undefined);
const b = { __proto__: null, only: 2 };
console.log(b.only, (b as any).toString === undefined, (b as any).hasOwnProperty === undefined);
// ordinary literal keeps the builtin surface
const c = { k: 3 };
console.log(c.k, typeof (c as any).toString);
console.log("survived");
