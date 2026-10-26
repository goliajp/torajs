// An array literal holding an `any` function, stored into a function
// array field: an assignment through a receiver, through `this`, and a
// class field's initializer. An object literal stored into a field
// whose type has a function member records it too.
type F = (x: number) => number;
const tag: any = (x: any) => "t" + x;
const quarter: any = (x: any) => x / 4;
class B {
  fs: F[] = [];
  set() {
    this.fs = [tag, (x: number) => x + 1];
  }
}
const b = new B();
b.fs = [quarter];
console.log(b.fs[0](1) + 1);
const c = new B();
c.set();
console.log(c.fs[0](2), c.fs[1](2));
class D {
  fs: F[] = [tag, (x: number) => x * 2];
  run(v: number): string {
    return this.fs.map((f) => String(f(v))).join("|");
  }
}
const d = new D();
console.log(d.fs[0](1), d.fs[1](1), d.run(5));
class Holder {
  cfg: { f: F } = { f: (x: number) => x };
}
const h = new Holder();
h.cfg = { f: tag };
console.log(h.cfg.f(7));
