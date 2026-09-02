// 562-07 — §15.7.14 ClassDefinitionEvaluation defines every class
// element in ONE ordered pass, so a computed member's own entry sits
// at its declaration position. tr evaluated a computed key at the
// class-decl position, long after the registration walk had already
// defined every plain member, and a dynobj entry can only be
// appended — so every computed member came out last.
const k1 = "c1";
const k2 = "c2";

class A { m() {} [k1]() {} n() {} }
console.log(JSON.stringify(Object.getOwnPropertyNames(A.prototype)));

// Computed first, plain after.
class B { [k1]() {} m2() {} }
console.log(JSON.stringify(Object.getOwnPropertyNames(B.prototype)));

// Two computed members with plain ones between and after.
class C { a() {} [k1]() {} b() {} [k2]() {} c() {} }
console.log(JSON.stringify(Object.getOwnPropertyNames(C.prototype)));

// An accessor pair under a computed key, with a plain method after.
class D { p() {} get [k1]() { return 1; } set [k1](v: number) {} q() {} }
console.log(JSON.stringify(Object.getOwnPropertyNames(D.prototype)));

// A plain accessor keeps its declaration position too (562-08).
class E { get g() { return 1; } [k1]() {} m() {} }
console.log(JSON.stringify(Object.getOwnPropertyNames(E.prototype)));

// The printed face reads the same object.
class F { m() {} get [k1]() { return 9; } n() {} }
console.log(new F());

// A subclass's own rows are its own, in its own element order.
class G extends A { x() {} [k2]() {} y() {} }
console.log(JSON.stringify(Object.getOwnPropertyNames(G.prototype)));

// Only computed members.
class H { [k1]() {} [k2]() {} }
console.log(JSON.stringify(Object.getOwnPropertyNames(H.prototype)));

// A plain accessor AFTER a computed member: the class-decl-position
// accessor reify redefines the same key, which keeps the position
// the ordered walk gave it.
class I { [k1]() {} get g() { return 1; } m() {} }
console.log(JSON.stringify(Object.getOwnPropertyNames(I.prototype)));

// (A GENERIC class's computed member does not land on the prototype
// at all — `["constructor","m","n"]` where bun answers
// `["constructor","m","c1","n"]`. That is 563-04, a separate gap:
// the reify never reaches the runtime, so there is nothing to order.)

// Instance and static computed members share one declaration-order
// numbering; the instance side must not be disturbed by the static.
class K { a() {} static [k1]() {} b() {} [k2]() {} c() {} }
console.log(JSON.stringify(Object.getOwnPropertyNames(K.prototype)));
