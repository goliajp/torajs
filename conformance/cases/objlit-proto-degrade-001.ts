// sec B.3.1 — a `__proto__: v` PropertyName field in an object
// literal is a [[Prototype]] set, not an own data field; the literal
// rides the dynobj lane so the chain is real.
const base = { m() { return 7; }, tagv: "base" };
const child = { __proto__: base, own: 1 };
console.log(child.own, child.m(), (child as any).tagv);
console.log(Object.getPrototypeOf(child) === base);
console.log(Object.prototype.hasOwnProperty.call(child, "__proto__"));
// the property SHORTHAND spelling is an ordinary own data field
const __proto__ = "shorthand-value";
const sh = { __proto__ };
console.log(Object.prototype.hasOwnProperty.call(sh, "__proto__"), (sh as any).__proto__);
// null proto: member miss answers undefined, chain is cut
const bare = { __proto__: null, only: 2 };
console.log(bare.only);
console.log("survived");
