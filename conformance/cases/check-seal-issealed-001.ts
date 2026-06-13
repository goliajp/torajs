// RFC C5b-3 — Object.seal / isSealed real bit + DynObj entry walk.
// Spec §10.1.4 / §20.1.2.15: a sealed object is non-extensible AND
// every own property is non-configurable. Prevent-only on a typed
// class instance is NOT sealed (own props remain configurable).

const e: any = {};
console.log("empty isSealed before", Object.isSealed(e));
Object.preventExtensions(e);
console.log("empty isSealed after prevent (vacuous true)", Object.isSealed(e));

const o: any = { x: 1, y: 2 };
console.log("o isSealed before", Object.isSealed(o));
Object.preventExtensions(o);
console.log("o isSealed after prevent (props still configurable)", Object.isSealed(o));
Object.seal(o);
console.log("o isSealed after seal", Object.isSealed(o));
console.log("o isExt after seal", Object.isExtensible(o));

class C { x = 1; y = 2 }
const c: any = new C();
console.log("c isSealed before", Object.isSealed(c));
Object.preventExtensions(c);
console.log("c isSealed after prevent", Object.isSealed(c));
const cret = Object.seal(c);
console.log("c isSealed after seal", Object.isSealed(c));
console.log("seal return identity", cret === c);

const a: any = [1, 2, 3];
console.log("a isSealed before", Object.isSealed(a));
Object.seal(a);
console.log("a isSealed after seal", Object.isSealed(a));

console.log("num isSealed", Object.isSealed(42));
console.log("str isSealed", Object.isSealed("hi"));
console.log("bool isSealed", Object.isSealed(true));
console.log("null isSealed", Object.isSealed(null));
console.log("undef isSealed", Object.isSealed(undefined));
