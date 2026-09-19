// A class expression in an INSTANCE field initializer evaluates per
// construction, inside the ctor, so its declaration now rides into the
// ctor prefix beside the `this.<f> = …` statement instead of the
// top-level splice: each instance gets its own class (identity per
// `new`), the body can capture the enclosing function's locals and read
// the enclosing class's statics, and a self-referencing inner class
// keeps its evaluation's identity. A STATIC field initializer keeps the
// definition-time splice.
class Outer {
  tag: string;
  inner = class { who(): string { return "inner"; } };
  constructor(tag: string) { this.tag = tag; }
}
const a = new Outer("a"), b = new Outer("b");
console.log(a.inner === b.inner, new a.inner().who(), a.inner.name);
class Outer2 {
  n: number;
  mk = class { self: Outer2; constructor(s: Outer2) { this.self = s; } n(): number { return this.self.n; } };
  constructor(n: number) { this.n = n; }
  make(): any { return new this.mk(this); }
}
console.log(new Outer2(7).make().n(), new Outer2(8).make().n());
function factory(k: number): any {
  class Holder { c = class { k(): number { return k; } }; }
  return new Holder();
}
console.log(new (factory(3).c)().k(), new (factory(4).c)().k());
class Outer3 { static count = 0; s = class { static id = ++Outer3.count; }; }
const x = new Outer3(), y = new Outer3();
console.log(x.s.id, y.s.id, Outer3.count);
class Outer4 { typed: any = class Named { v(): number { return 4; } }; st = class extends Outer { constructor() { super("st"); } }; }
const o4 = new Outer4();
console.log(new o4.typed().v(), o4.typed.name, new o4.st().tag, new o4.st() instanceof Outer);
class Outer5 { static k = class { who(): string { return "static"; } }; inst = class { who(): string { return "inst"; } }; }
console.log(new Outer5.k().who(), Outer5.k === Outer5.k, new (new Outer5().inst)().who());
