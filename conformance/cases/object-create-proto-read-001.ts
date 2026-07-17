// §10.1.8.1 OrdinaryGet over the user [[Prototype]] chain (RFC
// 20260717-user-proto-chain knife 2) — inherited data reads, method
// dispatch with this = child, grandparent recursion, own-property
// shadowing, and the null-proto no-inherit shape.

const parent: any = { kind: "animal", n: 42 };
const child: any = Object.create(parent);
console.log(child.kind); // animal
console.log(child.n + 1); // 43

// own property shadows the chain
child.kind = "dog";
console.log(child.kind); // dog
console.log(Object.getPrototypeOf(child).kind); // animal

// grandparent recursion
const leaf: any = Object.create(child);
console.log(leaf.n); // 42
console.log(leaf.kind); // dog

// full-chain miss stays undefined
console.log(leaf.missing); // undefined

// inherited method runs with this = receiver
const proto2: any = { greet() { return "hi " + this.name; } };
const c2: any = Object.create(proto2);
c2.name = "tora";
console.log(c2.greet()); // hi tora

// inherited method through two levels
const c3: any = Object.create(c2);
c3.name = "leaf";
console.log(c3.greet()); // hi leaf

// null-proto dict inherits nothing
const dict: any = Object.create(null);
console.log(dict.kind); // undefined

// the builtin Object.prototype surface still reifies below the chain
console.log(typeof child.hasOwnProperty); // function
console.log(child.hasOwnProperty("kind")); // true
console.log(child.hasOwnProperty("n")); // false
console.log("done");
