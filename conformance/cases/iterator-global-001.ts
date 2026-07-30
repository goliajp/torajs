// RFC 20260730-iterator-global 刀 1 — the `Iterator` global identity:
// typeof / name / prototype readback, §27.1.3.1 abstract-ctor
// TypeError, `class C extends Iterator` (ordinary instance +
// prototype chain through %Iterator.prototype%), and §7.3.22
// instanceof membership for generator objects and builtin iterator
// cells.

console.log(typeof Iterator);
console.log(Iterator.name);
console.log(typeof Iterator.prototype);

// §27.1.3.1 — the Iterator constructor is abstract.
try {
  new Iterator();
  console.log("no-throw");
} catch (e) {
  console.log(e instanceof TypeError);
}

// extends Iterator — ordinary instance, chained prototype.
class SubIterator extends Iterator {
  next() {
    return { value: undefined, done: true };
  }
}
const s = new SubIterator();
console.log(s instanceof SubIterator);
console.log(s instanceof Iterator);
console.log(Object.getPrototypeOf(SubIterator.prototype) === Iterator.prototype);

// Generator objects sit on the %GeneratorPrototype% →
// %Iterator.prototype% chain (§27.1.2).
function* g() {
  yield 1;
}
const it = g();
console.log(it instanceof Iterator);
console.log(it.next().value);

// Builtin iterator cells are Iterator instances. (A string's
// symbol-indexed iterator call — `"ab"[Symbol.iterator]()` — is an
// independent pre-existing gap, recorded in the RFC's boundary
// list, not exercised here.)
console.log([].values() instanceof Iterator);
console.log(new Map().entries() instanceof Iterator);

// Non-iterators are not.
const plain: any = {};
console.log(plain instanceof Iterator);
console.log([1] instanceof Iterator);
