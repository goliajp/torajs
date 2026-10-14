// A generic class body never lowers: the mono pass clones a body per
// instantiation, and only those run. The shadow census over the mono
// program sees the clones' writes; a write of a value the member's
// slot cannot hold has to reach the typed reads of the member even
// though no run over the generic program saw it, and a typed store the
// clone makes (`this.v = x` with `x: number`) shadows nothing.
class Box<T> {
  n: number = 0;
  v!: T;
  set(x: T) {
    (this as any).n = x;
    this.v = x;
  }
}
const b = new Box<string>();
b.set("s");
console.log(b.n, typeof b.n, b.v);
const c = new Box<number>();
c.set(5);
console.log(c.n + 1, c.v + 1);

class Pair<T> {
  a: number = 1;
  b: number = 2;
  swap(x: T) {
    const t: any = this;
    t.a = x;
    this.b = this.b + 1;
  }
}
const p = new Pair<boolean>();
p.swap(true);
console.log(p.a, p.b, typeof p.a);
const q = new Pair<number>();
q.swap(9);
console.log(q.a + q.b);
