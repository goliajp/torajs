// An inline object literal installed as a [[Prototype]] is read with
// every inheritor as its receiver: super property reads and writes,
// inherited methods, accessors and data fields all resolve on the
// literal while `this` stays the inheriting object.
class B {
  _s = 7;
  set s(v: number) { this._s = v * 10 }
  get g() { return this._s * 10 }
}
class D extends B {
  write() { super.s = 4; return this._s }
  read() { return super.g }
  data() { return super.x }
}
Object.setPrototypeOf(D.prototype, {
  x: 5,
  set s(v: any) { (this as any)._s = v + 100 },
  get g() { return (this as any)._s + 100 },
  m() { return this },
});
const d = new D();
console.log(d.write(), d.read(), d.data());
console.log((d as any).x, (d as any).m() === d);

class E extends D { write() { super.s = 1; return this._s } }
console.log(new E().write());

// Reflect.setPrototypeOf takes the same argument.
class F { _s = 1; m() { return (this as any).twice() } }
console.log(Reflect.setPrototypeOf(F.prototype, { twice() { return (this as any)._s * 2 } }), new F().m());

// Object.create with an inline literal proto.
const o = Object.create({
  get g() { return (this as any)._s + 100 },
  set s(v: any) { (this as any)._s = v },
  k: 9,
});
o._s = 1;
console.log(o.g, o.k);
o.s = 5;
console.log(o._s, o.g, Object.keys(o).join(","));
