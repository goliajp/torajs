// RFC 20260815 knife 2 — a non-Ident heritage expression extracts to
// a value binding; the class lowers on the capturing lane and its
// super(...) dispatches at run time through the parent VALUE (a class
// cell routes to its registered __ctorany_ twin).
class Base {
  v: number;
  constructor(p) {
    this.v = p;
  }
  m() {
    return this.v * 10;
  }
}
function pick() {
  return Base;
}
class K1 extends pick() {
  w: number;
  constructor() {
    super(1);
    this.w = 5;
  }
  sum() {
    return this.m() + this.w;
  }
}
const k1 = new K1();
console.log(k1.sum(), k1.v, k1 instanceof K1, k1 instanceof Base);
// member-expression heritage
const box = { cls: Base };
class K2 extends box.cls {
  constructor() {
    super(2);
  }
}
console.log(new K2().m(), new K2() instanceof Base);
// as-cast heritage
class K3 extends (Base as any) {
  constructor() {
    super(3);
  }
}
console.log(new K3().m());
// arbitrary expression heritage on a class expression, implicit ctor
const K4 = class extends [Base][0] {};
console.log(new K4(4).m(), new K4(4) instanceof Base);
// heritage expression evaluates once, at definition time
let calls = 0;
function mk() {
  calls++;
  return Base;
}
class K5 extends (mk() as any) {
  constructor() {
    super(9);
  }
}
new K5();
new K5();
console.log(calls);
