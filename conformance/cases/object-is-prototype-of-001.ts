// §20.1.3.3 Object.prototype.isPrototypeOf (RFC
// 20260717-user-proto-chain knife 4) — identity walk over the user
// chain, multi-level, primitives false, builtin prototypes visible.

const a: any = { tag: "a" };
const b: any = Object.create(a);
const c: any = Object.create(b);

console.log(a.isPrototypeOf(b)); // true
console.log(a.isPrototypeOf(c)); // true
console.log(b.isPrototypeOf(c)); // true
console.log(c.isPrototypeOf(a)); // false
console.log(b.isPrototypeOf(a)); // false
console.log(a.isPrototypeOf(a)); // false

// primitives are never on a chain
console.log(a.isPrototypeOf(5)); // false
console.log(a.isPrototypeOf("x")); // false
console.log(a.isPrototypeOf(null)); // false
console.log(a.isPrototypeOf(undefined)); // false

// unrelated object
console.log(a.isPrototypeOf({ tag: "z" })); // false

// re-parent moves the answer
const d: any = Object.create(a);
Object.setPrototypeOf(d, b);
console.log(a.isPrototypeOf(d)); // true (through b)
Object.setPrototypeOf(d, null);
console.log(a.isPrototypeOf(d)); // false

// builtin prototype singletons sit on the chain
console.log((Object.prototype as any).isPrototypeOf(b)); // true
console.log((Array.prototype as any).isPrototypeOf([1])); // true
console.log((Array.prototype as any).isPrototypeOf(b)); // false
console.log("done");
