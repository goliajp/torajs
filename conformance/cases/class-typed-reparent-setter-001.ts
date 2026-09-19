// A typed setter write on a re-parented class instance runs the user
// chain's setter, or creates the own property when the chain has none.
class C {
  _v = 0;
  set v(n: number) { this._v = n * 10; }
  get v() { return this._v; }
}
class D {
  _v = 0;
  set v(n: number) { this._v = n + 1000; }
  get v() { return this._v; }
}
const c = new C();
c.v = 1;
console.log(c.v, c._v);
Object.setPrototypeOf(c, D.prototype);
c.v = 2;
console.log(c.v, c._v);
const plain = new C();
Object.setPrototypeOf(plain, { k: 1 });
plain.v = 3;
console.log(Object.getOwnPropertyNames(plain).join(","), (plain as any).v);
