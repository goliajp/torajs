// 562-05 — bun's inspect walks up to five [[Prototype]] hops and
// prints what it finds there (`bindings.cpp` forEachProperty:
// `prototypeCount < 5`, stopping at Object.prototype /
// Function.prototype), with a key a nearer object carries belonging
// to that object (`visitedProperties`). tr's dynobj walker printed
// own entries only.
//
// A prototype that is a typed object literal (a struct cell, not a
// dynobj) is a separate gap (562-10), as is a builtin prototype
// (562-11) — so every prototype here is dynobj-shaped.
const base: any = { a: 1, m() {} };
const o: any = Object.create(base);
o.b = 2;
console.log(o);
console.log(Object.create(base));
// The nearer object's key wins.
const shadow: any = Object.create(base);
shadow.a = 99;
console.log(shadow);
// Three hops.
console.log(Object.create(Object.create(Object.create(base))));

class K { m() {} get g() { return 1; } }
console.log(Object.create(K.prototype));
class Sub extends K { n() {} }
console.log(Sub.prototype);
console.log(K.prototype);

// A null-prototype object above an ordinary one: the rows come
// through, the prefix belongs to the object being printed.
const np: any = Object.create(null);
np.x = 1;
console.log(Object.create(np));
console.log(np);

// The walk stops at %Object.prototype%.
console.log(Object.create(Object.prototype));
console.log({ a: 1 });

// An inherited `@@toStringTag` names the block and does not print as
// a row (the fast walk hides it; the hop below has other rows, so no
// slow restart).
const tagged: any = Object.create({ [Symbol.toStringTag]: "P", y: 2 });
console.log(tagged);
