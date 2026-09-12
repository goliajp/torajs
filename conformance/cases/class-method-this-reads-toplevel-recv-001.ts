// A method whose body reads `this` gets a receiver-polymorphic twin
// (RFC 20260804-method-rebind-generic-body), and the twin restored
// EVERY static `__cm_<C>__<m>(recv)` call in the cloned body to its
// member-call shape — sound for `this` and for parameters, which the
// twin retypes `any`, wrong for a module-level binding the method
// never binds: `g0` keeps its static class in the twin, and a
// member call on a typed receiver has no lowering lane. `class K {
// m() { this.x; return g0.get() } }` died in ssa-lower with
// "unsupported member call shape: get", and so did every generator
// body doing the same (`__Gen_<f>.next` always reads `this`). A
// this-free method never minted a twin, which is why the plain
// `function f() { return g0.get() }` shape always worked.
class Box {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
  get(): number {
    return this.v;
  }
}
let g0 = new Box(7);
const c0 = new Box(8);

class K {
  x = 1;
  m(): number {
    this.x = this.x + 1;
    return g0.get() + this.x;
  }
  // a parameter receiver still rides the any lane in the twin
  p(b: Box): number {
    this.x = this.x + 1;
    return b.get() + this.x;
  }
  // a local bound from a parameter too
  q(b: Box): number {
    this.x = this.x + 1;
    const t = b;
    return t.get() * this.x;
  }
}
const k = new K();
console.log("method", k.m(), "param", k.p(c0), "local", k.q(g0));

function* gen(): any {
  const first = c0.get();
  yield first;
  return g0.get();
}
const it = gen();
console.log("gen", it.next().value, it.next().value);

g0 = new Box(70);
console.log("reassigned", k.m(), gen().next().value, [...gen()].join(","));
