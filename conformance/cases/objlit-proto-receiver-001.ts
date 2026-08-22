// A literal installed as somebody's [[Prototype]] has its methods
// invoked with the INHERITING object as receiver, so its receiver
// face cannot be the literal's own struct shape.
var parent = { getThis: function () { return this; }, tag: "p" };
var a = { k: 1 };
Object.setPrototypeOf(a, parent);
console.log((a as any).getThis() === a);

var b = { k: 2, m() { return (this as any).getThis(); } };
Object.setPrototypeOf(b, parent);
console.log(b.m() === b);

// The same object read back through Object.create.
var made: any = Object.create(parent);
made.k = 3;
console.log(made.getThis() === made, made.tag);

// __proto__ in a literal installs it too (§B.3.1).
var viaProto: any = { __proto__: parent, k: 4 };
console.log(viaProto.getThis() === viaProto);

// The receiver a super site carries is the call site's, and the
// method it reaches is on the same widened prototype.
var withSuper = { k: 5, method() { return super["getThis"](); } };
Object.setPrototypeOf(withSuper, parent);
console.log(withSuper.method() === withSuper);
