// a read through the base type may run the base getter even when a
// subclass redeclares only the setter: a binding read across it must
// keep its own stake
class N {
  n: number;
  constructor(n: number) { this.n = n; }
}
class Holder {
  k: N = new N(1);
}
const h = new Holder();
const keep: N[] = [];
class B {
  get v(): number {
    h.k = new N(99);
    for (let i = 0; i < 4; i++) keep.push(new N(42));
    return 5;
  }
}
class D extends B {
  set v(x: number) {}
}
function f(b: B): number {
  const old = h.k;
  const x: any = b.v;
  return old.n + (x === undefined ? 0 : x);
}
console.log(f(new B()));
console.log(f(new D()));
console.log(f(new B()));
