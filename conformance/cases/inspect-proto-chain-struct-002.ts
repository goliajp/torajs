// 562-10 — a prototype that is a TYPED object literal is a struct
// cell, not a dynobj: its rows live in `torajs-meta::struct_print`,
// whose only entry printed a whole `Name { … }` block. bun's inspect
// does not care which cell shape holds the prototype — it walks up to
// five [[Prototype]] hops and prints what it finds — so the dynobj
// walk stopped at the first struct hop and objects whose whole
// content came from a struct prototype printed `{}`.
//
// Companion of `inspect-proto-chain-001.ts`, which keeps every
// prototype dynobj-shaped.
const b1 = { z: 1, w: 2 };
console.log(Object.create(b1));
// The nearer object's key wins over the struct prototype's.
const shadow: any = Object.create(b1);
shadow.z = 9;
console.log(shadow);
// An own key the prototype does not carry, printed first.
const own: any = Object.create(b1);
own.b = 3;
console.log(own);
// `__proto__` assignment: the right-hand literal is a struct too.
const viaProto: any = {};
viaProto.__proto__ = { q: 2 };
console.log(viaProto);
// A struct at the end of a dynobj chain.
const mid: any = Object.create(b1);
mid.x = 5;
console.log(Object.create(mid));
// An empty struct prototype contributes no rows.
console.log(Object.create({}));
// A class instance as a prototype: its own fields AND the methods its
// class prototype declares.
class C1 { f = 1; m() {} }
console.log(Object.create(new C1()));
// An accessor declared on the struct prototype's class.
class C2 { get g() { return 1; } }
console.log(Object.create(new C2()));
// The struct cell printed on its own is unchanged.
console.log(b1);
console.log(new C1());
