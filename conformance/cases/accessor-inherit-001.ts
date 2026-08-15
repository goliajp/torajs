// inherited accessors — §10.1.7/§10.1.9 [[Get]]/[[Set]] walk the
// prototype chain, so a subclass reads and writes a parent's
// accessor pair (rotation 413 blade 2: the accessor tables were
// probed with the receiver's own class name only, no parent walk).
class A {
  _x: number = 1;
  get x(): number { return this._x; }
  set x(v: number) { this._x = v + 1; }
  get ro(): string { return "ro:" + this._x; }
}
class B extends A {}
class C extends B {
  bump(): number { this.x = 10; return this.x; }
}
const b = new B();
console.log(b.x);
b.x = 5;
console.log(b.x);
console.log(b._x);
console.log(b.ro);
// two hops + accessor use inside a grandchild method body
const c = new C();
console.log(c.bump());
console.log(c.ro);
