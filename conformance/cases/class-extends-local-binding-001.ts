// 393-04 tail — `extends` a VALUE binding that aliases a class: the
// heritage Ident names no class declaration, so it extracts to a
// `__ccp<N>` binding and takes the value-shaped-parent lane.
class Real {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
  m(): number {
    return this.v * 2;
  }
  static tag(): string {
    return "real";
  }
}

// block-scoped let alias
{
  let B = Real;
  class K extends B {
    constructor() {
      super(21);
    }
  }
  const k = new K();
  console.log(k.m(), k instanceof Real);
}

// top-level const alias; subclass adds a method; statics ride the
// class-side prototype link through the alias
const P = Real;
class T extends P {
  constructor() {
    super(5);
  }
  n(): number {
    return this.v + 1;
  }
}
const t = new T();
console.log(t.m(), t.n(), t instanceof Real, T.tag());

// implicit ctor forwards through the alias
{
  const B2 = Real;
  class M extends B2 {}
  const m = new M(7);
  console.log(m.m(), m instanceof Real);
}

// alias inside a function body
function make() {
  const B3 = Real;
  class Q extends B3 {
    constructor() {
      super(3);
    }
  }
  return new Q();
}
console.log(make().m());

// alias of a DERIVED class — the super chain composes through the
// value dispatch
class Mid extends Real {
  b(): number {
    return this.v + 100;
  }
}
const PM = Mid;
class Leaf extends PM {
  k(): number {
    return 9;
  }
}
const x = new Leaf(4);
console.log(x.m(), x.b(), x.k(), x instanceof Real, x instanceof Mid);
