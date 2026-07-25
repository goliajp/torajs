// A base class's field initializers did not run for a subclass
// instance. `class B { v: number = 5 }` / `class D extends B { e = 6 }`
// answered `d.v === 0`, with no method in sight — and a base ctor's
// parameters had nowhere to arrive, so `new D(9)` was rejected as
// "expected 0 argument(s), got 1".
//
// A class with field initializers and no `constructor` gets one from
// the parser, purely to hold them. That ctor then stopped the derived
// default-ctor synthesis at the door, so the class was left with no
// `super(...)` at all. Per §15.7.14 such a class simply has the
// implicit default ctor — and that one does call super.
//
// The initializers also belong AFTER the super call (§10.2.11: they
// run once `this` exists), which is what lets one of them read an
// inherited field.

class Base {
  v: number = 5;
  tag: string = "b";
}
class Derived extends Base {
  e: number = 6;
}

// a base whose ctor takes arguments: the derived class forwards them
class Sized {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
}
class Tagged extends Sized {
  label: string = "t";
}

// an initializer reading an inherited field — only correct if it runs
// after the super call
class Reader extends Base {
  doubled: number = this.v * 2;
}

// a user-written ctor keeps its own super, and the initializers land
// just after it rather than in front
class Explicit extends Base {
  extra: number = this.v + 1;
  constructor() {
    super();
    this.tag = this.tag + "!";
  }
}

// three levels, none of which declares a ctor
class L1 {
  a: number = 1;
}
class L2 extends L1 {
  b: number = 2;
}
class L3 extends L2 {
  c: number = 3;
}

// and three levels where the root takes an argument
class R1 {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}
class R2 extends R1 {
  y: number = 20;
}
class R3 extends R2 {
  z: number = 30;
}

// the shapes that already worked, kept as ground: a subclass with no
// fields of its own, an all-explicit chain, and a base with no ctor
// and no fields
class Bare extends Base {}
class Empty {}
class FromEmpty extends Empty {
  q: number = 8;
}

function main(): void {
  const d = new Derived();
  console.log(d.v, d.tag, d.e);

  const t = new Tagged(9);
  console.log(t.n, t.label);

  console.log(new Reader().doubled);

  const ex = new Explicit();
  console.log(ex.v, ex.tag, ex.extra);

  const l = new L3();
  console.log(l.a, l.b, l.c);

  const r = new R3(7);
  console.log(r.x, r.y, r.z);

  const bare = new Bare();
  console.log(bare.v, bare.tag);

  console.log(new FromEmpty().q);

  // `C.length` of a class holding only field initializers is the
  // implicit default ctor's, which is rest-shaped and therefore 0
  console.log(Derived.length, Tagged.length, Sized.length);
}

main();
