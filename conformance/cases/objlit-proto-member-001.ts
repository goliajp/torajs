// §13.2.5.5 — the `__proto__: v` object-literal member sets
// [[Prototype]] instead of defining an own property (RFC
// 20260717-user-proto-chain). Cell links, null marks the no-inherit
// shape, primitives are silently ignored.

const parent: any = { greet: "hi", n: 7 };
const o: any = { __proto__: parent, own: 1 };
console.log(o.own); // 1
console.log(o.greet); // hi
console.log(Object.getPrototypeOf(o) === parent); // true
console.log(Object.keys(o).length); // 1
console.log(o.hasOwnProperty("greet")); // false

// null proto literal
const n: any = { __proto__: null, k: 2 };
console.log(Object.getPrototypeOf(n)); // null
console.log(n.k); // 2

// primitive value is ignored — the implicit chain stays
const ig: any = { __proto__: 42 };
console.log(Object.getPrototypeOf(ig) === Object.prototype); // true
console.log(Object.keys(ig).length); // 0

// inherited method via the literal link, this = receiver
const pm: any = { tag() { return "t" + this.x; } };
const c: any = { __proto__: pm, x: 9 };
console.log(c.tag()); // t9
console.log("done");
