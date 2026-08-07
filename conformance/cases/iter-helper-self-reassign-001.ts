// `let it = g(); it = it.filter(cb)` — the iterator-helper
// self-transform, reassigned into the binding that held the source.
//
// tr types an unannotated `let` from its init, so the slot was the
// synthesized `__Gen_*` class and the helper cell (an Any) was a
// checker reject: "assignment to `iter` mismatch — slot is Obj but
// value is Any". The mutable-let widen pass (RFC 20260804) existed
// for exactly this, but classified neither end: a generator-factory
// call is a plain `Call(Ident)` (the `__new_` prefix rule misses it —
// the factory keeps the user's name), and the rhs is a method call on
// a non-namespace receiver. Both classify now: the init through the
// factory table (FnDecls whose return type names `__Gen_*`), the rhs
// as the self-receiver iterator transform, kept narrow to
// `<name> = <name>.{filter|map|flatMap|take|drop}(...)`.

function* g() {
  yield 1;
  yield 2;
  yield 3;
}

// filter, reassigned into its own source binding
let iter = g();
iter = iter.filter(function (v: any, c: any) {
  return v % 2 === 1;
});
console.log(iter.next().value);
console.log(iter.next().value);
console.log(iter.next().done);

// map, same shape
let it2 = g();
it2 = it2.map(function (v: any) {
  return v * 10;
});
console.log(it2.next().value);

// chained transform after the widen — the slot is any now
let it3 = g();
it3 = it3.filter(function (v: any) {
  return v > 1;
});
it3 = it3.map(function (v: any) {
  return v + 100;
});
console.log(it3.next().value);

// a this-reading callback in the reassigned transform (the shape the
// three test262 *-this cases hold after the cb-this knife)
let it4 = g();
it4 = it4.filter(function (v: any, count: any) {
  return this === undefined && v < 3;
});
console.log(it4.next().value, it4.next().value, it4.next().done);

// an untouched generator binding keeps its typed lane
let keep = g();
console.log(keep.next().value);

// a mutable binding with no cross-family reassign keeps its lane too
let n = 1;
n = n + 1;
console.log(n);
