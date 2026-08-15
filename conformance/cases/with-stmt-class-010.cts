// RFC 20260815 knife 3 / RFC 20260814 residue — an `extends` clause
// inside a `with` body takes the standard read guard: §15.7.14
// evaluates the heritage in the with scope, so the object can supply
// the parent. The guarded clause is an expression, which knife 2's
// value-parent lane dispatches at run time.
class Real {
  m() {
    return "real";
  }
}
class Fake {
  m() {
    return "fake";
  }
}
const o: any = { Base: Fake };
class Base extends Real {}
with (o) {
  class K extends Base {}
  const k = new K();
  console.log(k.m(), k instanceof Fake, k instanceof Base);
}
with ({}) {
  class K2 extends Base {}
  console.log(new K2().m(), new K2() instanceof Base);
}
// super carries arguments through the guarded parent
class P2 {
  v: number;
  constructor(x) {
    this.v = x * 2;
  }
}
with ({ P2: Real }) {
  class K4 extends P2 {
    constructor() {
      super(21);
    }
  }
  console.log(new K4() instanceof Real);
}
