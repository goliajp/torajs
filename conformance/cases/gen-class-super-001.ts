// P-SURF S2.11 — `super.m()` inside a class generator method.
//
// The parser resolves the `super` marker while the parent is still in
// hand (rotation 226), rewriting it to `__cm_<Parent>__<m>(recv, …)`.
// What was left open was the receiver's type: the hoisted generator
// took it as `any`, so the rewritten call — whose first parameter is
// the parent's nominal type — was rejected at the boundary.
//
// The receiver is now annotated with the declaring class's own name.
// An inherited generator still reaches the right receiver because the
// forwarder that passes `this` lives on the declaring class, and the
// call-site receiver slot already admits a subclass by prefix layout.
// `static *g()` keeps `any`: there the receiver is the class object,
// not an instance.

class A {
  base: number = 7;
  m(x: number): number {
    return x + 1;
  }
  static sm(): number {
    return 42;
  }
  *ag() {
    yield this.base;
  }
}

class B extends A {
  // the plain shape, plus a parameter that has to keep its slot behind
  // the synthesized receiver
  *g(a: number) {
    yield super.m(a);
    // twice in one expression, and mixed with `this`
    yield super.m(a) * 2;
    yield this.base;
  }

  // `super` and `this` interleaved across several yields
  *chain() {
    yield super.m(0);
    yield this.base;
    yield super.m(0) + this.base;
  }

  // a static generator still carries an `any` receiver
  static *sg() {
    yield A.sm();
  }
}

class C extends B {
  *h() {
    yield this.base + 100;
  }
}

const b = new B();
console.log([...b.g(3)]);
console.log([...b.chain()]);
console.log([...b.ag()]);
console.log([...B.sg()]);

// a grandchild instance reaching a generator declared two levels up,
// and one declared one level up — the direction that would break if the
// receiver were typed nominally without the subclass admit
const c = new C();
console.log([...c.h()]);
console.log([...c.g(1)]);
console.log([...c.ag()]);

// stepping by hand still reports `done` per the protocol
const it = b.g(0);
console.log(it.next().value, it.next().value, it.next().value, it.next().done);

// per-instance state survives across next() calls with `super` in play
const b2 = new B();
b2.base = 100;
console.log([...b2.g(1)], [...b.g(1)]);

console.log(A.sm());
