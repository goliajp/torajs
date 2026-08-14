// 405-01 substrate — a FUNCTION value's user [[Prototype]] chain.
// `Object.setPrototypeOf(D, P)` re-parents a function value through
// its lazy expando dynobj (the same `\x00proto` simulation entry a
// dynobj receiver carries), which is the class-side static-
// inheritance face the ES5 extends pattern needs.

// method + value reads through the link, plus identity and `in`
const P: any = function () {};
P.s = function (): number { return 2 };
P.tagd = 7;
const D: any = function () {};
Object.setPrototypeOf(D, P);
console.log(D.s());
console.log(D.tagd);
console.log(Object.getPrototypeOf(D) === P);
console.log("s" in D, "nope" in D);

// a static added to the parent AFTER the link flows down
P.late = function (): number { return 9 };
console.log(D.late());

// receiver identity: an inherited method's `this` is the receiver
const P2: any = function () {};
P2.self = function (): any { return this };
const C2: any = function () {};
Object.setPrototypeOf(C2, P2);
const D2: any = function () {};
Object.setPrototypeOf(D2, C2);
console.log(D2.self() === D2);

// grandparent chain identity
console.log(Object.getPrototypeOf(Object.getPrototypeOf(D2)) === P2);

// a cycle refuses with TypeError
try {
  Object.setPrototypeOf(P2, D2);
  console.log("no throw");
} catch (e) {
  console.log("cycle threw");
}

// Reflect flavor answers the boolean and takes effect
console.log(Reflect.setPrototypeOf(D2, P2));
console.log(Object.getPrototypeOf(D2) === P2);

// an own entry shadows the inherited one
D2.self = function (): string {
  return "own";
};
console.log(D2.self());

// explicit null ends the chain
Object.setPrototypeOf(D2, null);
console.log(Object.getPrototypeOf(D2));
