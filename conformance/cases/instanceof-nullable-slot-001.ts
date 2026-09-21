// §7.3.22 OrdinaryHasInstance step 3 — the answer is false when V is
// not an Object, and a slot the checker typed `T | undefined` is not
// one whenever it is holding its undefined. The compile-time fold
// reads only the SSA type, which is the shape the slot COULD hold,
// so `xs instanceof Array` came out as a constant `true`.

const o: { a: number } | undefined = undefined;
console.log(o instanceof Object);

const xs: number[] | undefined = undefined;
console.log(xs instanceof Array);
console.log(xs instanceof Object);

class C {
  a = 1;
}
const c: C | undefined = undefined;
console.log(c instanceof C);
console.log(c instanceof Object);

// (a fn-typed slot spells its undefined with the Str-shaped
// oddball rather than the generic cell, and the any box has no
// arm for `FnSig` at all — `f instanceof Function` still folds to
// a constant true. Registered, a different in-band spelling.)

const d: Date | undefined = undefined;
console.log(d instanceof Date);

// --- the same slots holding a live value answer the real walk
const o2: { a: number } | undefined = { a: 1 };
console.log(o2 instanceof Object);
const xs2: number[] | undefined = [1];
console.log(xs2 instanceof Array);
console.log(xs2 instanceof Object);
const c2: C | undefined = new C();
console.log(c2 instanceof C);
console.log(c2 instanceof Object);
const d2: Date | undefined = new Date(0);
console.log(d2 instanceof Date);

// a live instance is still not a member of an unrelated class
class D {
  b = 2;
}
console.log(c2 instanceof D);

// --- the non-nullable spellings keep the constant fold
console.log({ a: 1 } instanceof Object);
console.log([1] instanceof Array);
console.log([1] instanceof Object);
console.log(new C() instanceof C);
console.log(7 instanceof Object);
console.log("s" instanceof String);
