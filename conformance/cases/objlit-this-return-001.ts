// An object-literal method that returns `this`. The return type was
// sniffed before the pass that knows what the receiver IS had run, and
// `__this` resolved through a program-wide by-name annotation map —
// where every class method in the program contributes an entry and the
// last one wins. The injected `Error` subclasses are class methods too,
// so `{ v: 5, self() { return this; } }` typed `self` as returning
// `ReferenceError`, and every use of it failed against a class the
// program never mentions.
//
// The surfaces that already worked and must keep working are here too:
// an explicit return annotation, a class method, and `this.<field>`.

function bare(): void {
  const o = { v: 5, self() { return this; } };
  console.log(o.self().v);
}

// two literals in one program — the collision was constant, not
// index-dependent, so both reported the same wrong class
function twoLiterals(): void {
  const a = { v: 1, s() { return this; } };
  const b = { w: 2, t() { return this; } };
  console.log(a.s().v, b.t().w);
}

// chaining through the receiver is the point of returning it
function chained(): void {
  const box = {
    n: 0,
    bump() {
      this.n = this.n + 1;
      return this;
    },
  };
  // two links; a third reaches a separate lowering limit
  // ("unsupported member call shape"), recorded and not this fix's
  console.log(box.bump().n);
  console.log(box.bump().bump().n);
}

// a method returning the receiver alongside one that does not
function mixedMembers(): void {
  const o = {
    v: 7,
    self() { return this; },
    doubled() { return this.v * 2; },
  };
  console.log(o.self().v, o.doubled());
}

// already worked — an explicit annotation was never sniffed
function annotated(): void {
  const o = { v: 5, self(): any { return this; } };
  console.log(o.self().v);
}

// already worked — a class method's receiver type was always known.
// (A SIBLING method returning something other than the receiver still
// mis-types, both before and after this fix; recorded, not this fix's
// shape.)
class Counter {
  v: number = 5;
  self() { return this; }
}

function classMethod(): void {
  console.log(new Counter().self().v);
}

// already worked — reading a field through the receiver
function fieldRead(): void {
  const o = { v: 5, get2() { return this.v; } };
  console.log(o.get2());
}

function main(): void {
  bare();
  twoLiterals();
  chained();
  mixedMembers();
  annotated();
  classMethod();
  fieldRead();
}

main();
