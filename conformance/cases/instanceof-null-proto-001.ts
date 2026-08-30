// §7.3.20 OrdinaryHasInstance walks the receiver's prototype chain,
// and a null-prototype object has none to walk — so it is NOT an
// instance of Object. tr answered true for every heap shape that was
// not a boxed primitive, which is right for every object that
// eventually reaches %Object.prototype% and wrong for the three that
// do not: `Object.create(null)`, a re-parented object, and a module
// namespace (§10.4.6.1 answers null).
//
// The re-parent pair is the point: the same cell answers false while
// its prototype is null and true once it has one again, so the answer
// tracks the bit rather than how the cell was born.
const bare: any = Object.create(null);
console.log(bare instanceof Object);

const plain: any = {};
console.log(plain instanceof Object);

const reparented: any = {};
Object.setPrototypeOf(reparented, null);
console.log(reparented instanceof Object);
Object.setPrototypeOf(reparented, {});
console.log(reparented instanceof Object);

const dict: any = Object.create(null);
dict.k = 1;
console.log(dict.k, Object.keys(dict).join(","), dict instanceof Object);
