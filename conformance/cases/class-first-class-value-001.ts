// RFC 20260717-class-first-class-value knife A — a class is a
// first-class function object: typeof through an indirect binding,
// [[Prototype]] → %Function.prototype%, and the §10.2.3
// MakeConstructor `prototype.constructor` back-link with the
// {writable, non-enumerable, configurable} attribute set.
class D {}
var C = class Q {};
console.log(typeof D);
console.log(typeof C);
console.log(D.name, C.name);
console.log(Object.getPrototypeOf(D.prototype) === Object.prototype);
console.log(Object.getPrototypeOf(D) === Function.prototype);
console.log(D.prototype.constructor === D);
console.log(C.prototype.constructor === C);
// constructor is non-enumerable — must not leak into keys / JSON.
console.log(Object.keys(D.prototype).length);
const desc = Object.getOwnPropertyDescriptor(D.prototype, "constructor");
console.log(desc.writable, desc.enumerable, desc.configurable);
class Sub extends D {}
console.log(Sub.prototype.constructor === Sub);
console.log(new Sub() instanceof D);
