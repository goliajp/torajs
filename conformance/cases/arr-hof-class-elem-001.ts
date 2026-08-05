// A higher-order array method over class instances. Pre-fix every one
// of these was a compile error:
//
//   argument 0: expected Function([Struct([("v", Number)]), Number,
//   Array(Any)], Boolean), got Function([ClassRef("Box")], Boolean)
//
// The array's element type arrives already resolved to its struct
// shape, while the arrow's inferred parameter keeps the class name, so
// the callback-subtype check compared a name against a shape. The
// top-level resolve that would have reconciled them does not reach
// inside a callback's parameter list.
//
// `map` was the one that already worked — its result type takes a
// different route — which is why the family looked healthier than it
// was.

class Box {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
  get(): number {
    return this.v;
  }
}

const bs = [new Box(1), new Box(2), new Box(3)];

// field read and method call inside the callback, across the family
console.log(bs.filter(b => b.v > 1).length);
console.log(bs.filter(b => b.get() > 1).length);
console.log(bs.map(b => b.get()).join(","));
console.log(bs.find(b => b.get() === 2) !== undefined);
console.log(bs.findLast(b => b.v < 3) !== undefined);
console.log(bs.findIndex(b => b.v === 3));
console.log(bs.findLastIndex(b => b.v === 1));
console.log(bs.some(b => b.v > 2), bs.every(b => b.v > 0));

bs.forEach(b => console.log("each", b.get()));

// the callback may still take the trailing spec parameters
console.log(bs.filter((b, i) => i > 0 && b.v > 0).length);
bs.forEach((b, i) => console.log("idx", i, b.v));

// subclass elements — a heterogeneous array whose common shape is the
// base class, the shape the original probe was built from
class Cup extends Box {
  constructor(v: number) {
    super(v * 10);
  }
}
const mixed = [new Box(1), new Cup(2)];
console.log(mixed.filter(m => m.v > 5).length);
console.log(mixed.map(m => m.get()).join(","));

// the Error family is the same shape: instances of a class hierarchy
const errs = [new Error("a"), new TypeError("b")];
console.log(errs.map(e => e.name).join(","));
console.log(errs.filter(e => e instanceof TypeError).length);
console.log(errs.some(e => e.message === "a"));
