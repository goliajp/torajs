// The factory seeds `__this` with a zero per declared field type, and
// that table knew six spellings — number, string, boolean, arrays,
// Map/Set/WeakMap/WeakSet, and class or alias types. Every other type
// took a catch-all `0`, so the seed literal disagreed with the class it
// was declared as and the factory died on its own synthesized object
// ("declared ClassRef(\"C\"), init has Struct([(\"d\", Number)])"),
// taking every other field of that class down with it. A class could
// not have a `Date`, `RegExp`, `bigint`, `symbol` or `WeakRef` field at
// all — however it was initialized, since the seed is built before the
// constructor runs.

class WithDate {
  d: Date = new Date(5);
}
console.log("date", new WithDate().d.getTime());

class DateNoInit {
  d: Date;
  n: number = 5;
}
console.log("date-uninitialized-sibling", new DateNoInit().n);

class DateFromCtor {
  d: Date;
  constructor(ms: number) {
    this.d = new Date(ms);
  }
}
console.log("date-from-ctor", new DateFromCtor(9).d.getTime());

class WithRegExp {
  r: RegExp = /ab+c/gi;
}
const wr = new WithRegExp();
console.log("regexp", wr.r.source, wr.r.flags, wr.r.test("xxABBBC"));

class WithBigInt {
  b: bigint = 7n;
}
console.log("bigint", new WithBigInt().b);

class BigIntFromCtor {
  b: bigint = 0n;
  constructor(v: bigint) {
    this.b = v;
  }
}
console.log("bigint-from-ctor", new BigIntFromCtor(12345678901234567890n).b);

class WithSymbol {
  s: symbol = Symbol("k");
}
console.log("symbol", typeof new WithSymbol().s);

class WithWeakRef {
  w: WeakRef<object> = new WeakRef({});
}
console.log("weakref", typeof new WithWeakRef().w);

// A parent's field of one of these types is flattened into the
// subclass's seed, and instances of such a class live in containers.
class Base {
  d: Date = new Date(11);
}
class Derived extends Base {
  n: number = 1;
}
const dv = new Derived();
console.log("inherited", dv.d.getTime(), dv.n);

const many: WithDate[] = [new WithDate(), new WithDate()];
console.log("in-array", many[0].d.getTime(), many.length);

// Several of them on one class, alongside the types the table already
// knew — every field has to agree for the seed to type at all.
class Mixed {
  n: number = 1;
  s: string = "x";
  b: boolean = true;
  d: Date = new Date(13);
  r: RegExp = /z/;
  big: bigint = 2n;
  xs: number[] = [4, 5];
  m: Map<string, number> = new Map();
}
const mx = new Mixed();
mx.m.set("k", 6);
console.log(
  "mixed",
  mx.n,
  mx.s,
  mx.b,
  mx.d.getTime(),
  mx.r.source,
  mx.big,
  mx.xs[1],
  mx.m.get("k"),
);

// The spellings the table already handled, unchanged.
class Known {
  n: number = 3;
  s: string = "y";
  b: boolean = false;
  xs: string[] = ["a"];
  st: Set<number> = new Set();
  u: number | null = null;
}
const kn = new Known();
kn.st.add(8);
console.log("known", kn.n, kn.s, kn.b, kn.xs[0], kn.st.has(8), kn.u);

class Inner {
  v: number = 2;
}
class Outer {
  i: Inner = new Inner();
}
console.log("class-typed-field", new Outer().i.v);
