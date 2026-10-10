// A generic class's constructor reads `arguments` while its `new`
// sites pass more arguments than it declares; each instantiation gets
// its own specialized constructor and factory, and both carry the
// count and the values of the `new` site's list.
class Box<T> {
  n: number;
  v: T;
  extra: any;
  constructor(v: T) {
    this.v = v;
    this.n = arguments.length;
    this.extra = arguments[2];
  }
}
const b = new Box(1, 2, 3);
console.log(b.n, b.v, b.extra);
const c = new Box("a");
console.log(c.n, c.v, c.extra);
const d = new Box<string>("x", "y", "z", "w");
console.log(d.n, d.v, d.extra);

class Count<T> {
  n: number;
  constructor(v: T) {
    this.n = arguments.length;
  }
}
console.log(new Count(1, 2, 3).n, new Count("s").n, new Count<boolean>(true, false).n);
