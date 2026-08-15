// construct-channel — a class whose ctor params are UNTYPED reaches
// the value-construct kernel: implicit generics used to genericize
// `__new_<C>` away from the bare spelling the ctor registry keys on,
// so `new nd(7)` died on "cannot be reached through a runtime value"
// whenever no static call site drove an instance (rotation 409).
class Box {
  v: number;
  constructor(p) {
    this.v = p;
  }
  m() {
    return this.v;
  }
}
const nd: any = Box;
const a: any = new nd(7);
const b: any = Reflect.construct(Box as any, [8]);
console.log(a.v, a.m(), b.v, b.m(), a instanceof Box, b instanceof Box);
// mixed static + dynamic sites share the one any-param factory
class Pair {
  s: number;
  constructor(x, y) {
    this.s = x + y;
  }
}
const st = new Pair(1, 2);
const dy: any = new (Pair as any)(3, 4);
console.log(st.s, dy.s, dy instanceof Pair);
