// a regular method spelled like an accessor face's symbol is not the
// accessor: reading `.v` through the base type runs the getter
class B {
  get v(): any { return 1; }
}
class D extends B {
  v_get(): string { return "m"; }
}
class E extends D {
  v_get(): string { return "e"; }
}
function read(b: B): any { return b.v; }
console.log(read(new B()), read(new D()), read(new E()));
const d: D = new E();
console.log(d.v_get(), d.v);
class S {
  _w = 0;
  set w(x: number) { this._w = x; }
}
class T extends S {
  w_set(): string { return "t"; }
}
class U extends T {
  w_set(): string { return "u"; }
}
function put(s: S, x: number): number { s.w = x; return s._w; }
console.log(put(new S(), 1), put(new T(), 2), put(new U(), 3));
const t: T = new U();
console.log(t.w_set());
