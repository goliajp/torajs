// wrapper receivers inherit symbol-keyed properties through their
// primitive's prototype singleton (member_get_symbol inherited_dict
// wrapper arm) — the test262 concat iterable-primitive-wrapper-
// objects face: defineProperty(Boolean.prototype, Symbol.iterator).
const desc: any = {
  value: function() { return { next: function() { return { done: true, value: undefined }; } }; },
  writable: false, enumerable: false, configurable: true,
};
Object.defineProperty(Boolean.prototype, Symbol.iterator, desc);
Object.defineProperty(Number.prototype, Symbol.iterator, desc);
Object.defineProperty(Symbol.prototype, Symbol.iterator, desc);
const b: any = Object(true);
console.log("a", b[Symbol.iterator]().next().done);
const n: any = Object(123);
console.log("b", n[Symbol.iterator]().next().done);
const sy: any = Object(Symbol("d"));
console.log("c", sy[Symbol.iterator]().next().done);
// string wrapper keeps its NATIVE @@iterator (spec: String.prototype
// already carries one)
const sw: any = Object("te");
const sit = sw[Symbol.iterator]();
console.log("d", sit.next().value, sit.next().value, sit.next().done);
// concat drives the whole family
const it = Iterator.concat(Object(true), Object(123), Object("xy"), Object(Symbol()));
console.log("e", it.next().value, it.next().value, it.next().done);
