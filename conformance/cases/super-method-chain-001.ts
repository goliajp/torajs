// P-SURF S2.12 — `super.m()` names the nearest ancestor that declares
// `m`, not the direct parent.
//
// The rewrite consulted the direct parent only, so
// `class C extends B extends A` reaching A's method emitted a call to
// `__cm_B__m` and the program died on an unknown identifier. Found while
// probing S2.11; not a generator defect, since the plain-method spelling
// failed identically.
//
// What the walk has to get right is *which* ancestor: the nearest one
// that declares the name, so an override in the middle of the chain
// still wins over the grandparent's version.

class A {
  tag(): string {
    return "A";
  }
  depth(): number {
    return 1;
  }
}

// declares nothing — the link that used to break the chain
class B extends A {}

class C extends B {
  // reaches two levels up
  tag(): string {
    return "C(" + super.tag() + ")";
  }
  // and so does a second method on the same class
  depth(): number {
    return super.depth() + 1;
  }
}

// four levels, with nothing declared in between
class D extends C {}
class E extends D {
  tag(): string {
    return "E(" + super.tag() + ")";
  }
}

// an override partway up: `super.tag()` from G must reach F, not A,
// because F is the nearer declaration
class F extends A {
  tag(): string {
    return "F";
  }
}
class G extends F {
  tag(): string {
    return "G(" + super.tag() + ")";
  }
}

// the direct-parent case, which is what already worked and must keep
// working
class H extends A {
  tag(): string {
    return "H(" + super.tag() + ")";
  }
}

// a constructor reaching a grandparent method, with arguments
class I extends A {
  greet(who: string, n: number): string {
    return "I:" + who + n;
  }
}
class J extends I {}
class K extends J {
  say(): string {
    return super.greet("k", 2) + "/" + super.tag();
  }
}

console.log(new C().tag(), new C().depth());
console.log(new E().tag());
console.log(new G().tag());
console.log(new H().tag());
console.log(new K().say());

// the walk does not disturb ordinary inherited dispatch: D declares
// nothing, so `d.tag()` is C's, which itself reaches A
const d = new D();
console.log(d.tag(), d.depth());

// `super.m()` from a *static* method is a separate open gap (S2.16) —
// it fails on the direct parent too, so it is not the chain walk — and
// is deliberately not exercised here.
