// P-SURF S2.9 — `super()` is an early SyntaxError anywhere but the body
// of a derived class's constructor (ES §15.7.1). The refusal itself is
// negative and lives in test262; what this fixture pins is the other
// side of that line — every position that stays **legal** — because the
// check is a parser flag cleared on entry to each function-like body,
// and the way to get it wrong is to clear it somewhere it should carry.

class A {
  tag: string = "A";
  constructor(t: string) {
    this.tag = t;
  }
  label(): string {
    return "A:" + this.tag;
  }
}

class B extends A {
  n: number = 0;
  constructor(t: string, n: number) {
    // the plain shape
    super(t);
    this.n = n;
  }
}

// an arrow has no `this` of its own, so it carries the constructor's
// position rather than clearing it
class C extends A {
  constructor(t: string) {
    const go = () => super(t);
    go();
  }
}

// nested arrows keep carrying it
class D extends A {
  constructor(t: string) {
    const go = () => () => super(t);
    go()();
  }
}

// a conditional super() is still one super()
class E extends A {
  constructor(t: string, alt: boolean) {
    if (alt) {
      super(t + "!");
    } else {
      super(t);
    }
  }
}

// super() inside a template interpolation, in a derived ctor — the
// sub-parser that handles interpolation tokens has to inherit the
// position, not restart from the default
class F extends A {
  msg: string = "";
  constructor(t: string) {
    super(t);
    this.msg = `${this.label()}/${t}`;
  }
}

// a method next door, and `super.m()` (which is legal in any method)
class G extends A {
  constructor(t: string) {
    super(t);
  }
  label(): string {
    return "G(" + super.label() + ")";
  }
  *gen() {
    yield super.label();
  }
}

// a class *expression* with its own derived constructor decides its own
// position. (The declaration spelling — `class Inner extends A {}` as a
// statement inside a function body — is a separate open gap, S2.13.)
const inner = new (class extends A {
  constructor() {
    super("inner");
  }
})();

console.log(new B("b", 3).tag, new B("b", 3).n);
console.log(new C("c").tag);
console.log(new D("d").tag);
console.log(new E("e", true).tag, new E("e", false).tag);
console.log(new F("f").msg);
console.log(new G("g").label());
console.log([...new G("g").gen()]);
console.log(inner.tag);
