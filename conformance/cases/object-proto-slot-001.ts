// §10.1.1 / Annex B §B.2.2.1 — an ordinary object's [[Prototype]] is
// %Object.prototype%, readable both as `Object.getPrototypeOf(o)` and
// (through the getter it inherits) as `o.__proto__`. tr couldn't tell
// a plain dynobj from `Object.create(null)` — both looked like "a
// dynobj with no __proto__ entry" — so getPrototypeOf answered null
// for every object, and the __proto__ read was an absent property.

const plain: any = { a: 1 };
console.log(Object.getPrototypeOf(plain) === Object.prototype);
console.log(plain.__proto__ === Object.prototype);
console.log(typeof plain.toString, typeof plain.hasOwnProperty);

// Object.create(null) really has no prototype, and does not inherit
// the __proto__ getter either: getPrototypeOf is null, the read is
// undefined, and "__proto__" is not "in" it.
const nul: any = Object.create(null);
nul.a = 1;
console.log(Object.getPrototypeOf(nul) === null);
console.log(nul.__proto__);
console.log("__proto__" in nul);
// (`typeof nul.toString` would be undefined in bun — a null-proto
// object inherits no methods — but tr's method lookup doesn't walk
// the __proto__ chain yet, a separate proto-chain-slot RFC.)

// Every receiver shape answers its constructor's prototype, through
// the typed lane and the primitive ToObject step alike.
console.log(([1] as any).__proto__ === Array.prototype);
const s: any = "x";
console.log(s.__proto__ === String.prototype);
const n: any = 5;
console.log(n.__proto__ === Number.prototype);
const b: any = true;
console.log(b.__proto__ === Boolean.prototype);
console.log((new Map() as any).__proto__ === Map.prototype);

// A builtin prototype's own [[Prototype]] is the root: every one of
// them chains up to Object.prototype, whose own is null.
console.log(Object.getPrototypeOf(Array.prototype) === Object.prototype);
console.log(Object.getPrototypeOf(Object.prototype) === null);
