// A class method whose value is taken out and called on a foreign
// receiver runs a receiver-polymorphic copy of its body; an optional
// call in that copy still short-circuits.
class C {
  v = 1;
  m(o: any) {
    return this.v + ":" + o?.f();
  }
  n(o: any) {
    return this.v + ":" + o?.g?.f(2);
  }
}
const c = new C();
const m: any = c.m;
const n: any = c.n;
console.log(c.m(null), c.m({ f: () => 3 }));
console.log(m.call({ v: 42 }, null), m.call({ v: 7 }, { f: () => 3 }));
console.log(n.call({ v: 42 }, undefined), n.call({ v: 8 }, { g: { f(x: any) { return x } } }));
