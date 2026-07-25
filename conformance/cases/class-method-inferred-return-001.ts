// A class method with no return annotation whose value comes off the
// receiver. The return-type sniff is a shape grammar with no type
// environment past the parameter annotations, so it cannot resolve a
// field read on `this`; the receiver-first arm was also the one arm
// that had no fallback, so the return stayed unset and the checker
// then expected Void and rejected the method:
//
//     return type mismatch: function expects Void, got Number
//
// The two sibling arms already fall back to `any` for exactly this
// reason. Reading a field is most of what methods do, so the arm
// without the fallback was turning away a large share of ordinary
// classes.

class Counter {
  n = 0;
  bump() {
    this.n = this.n + 1;
    return this;
  }
  value() {
    return this.n;
  }
}

class Labelled {
  tag = "hi";
  size: number = 3;
  read() {
    return this.tag;
  }
  readSized() {
    return this.size;
  }
  both() {
    return this.tag + this.size;
  }
}

// a method reading a field through another method, and one whose
// return the sniff CAN type (a literal) sitting beside them
class Mixed {
  base = 10;
  raw() {
    return 7;
  }
  scaled() {
    return this.base * 2;
  }
  viaOther() {
    return this.scaled() + this.raw();
  }
}

// branches: every path returns a field, and the ternary form
class Branching {
  a = 1;
  b = 2;
  pick(flag: boolean) {
    if (flag) {
      return this.a;
    }
    return this.b;
  }
  ternary(flag: boolean) {
    return flag ? this.a : this.b;
  }
}

// A subclass reading its OWN field. Reading an INHERITED one is left
// out on purpose: a field initializer declared on the base does not run
// for a subclass instance at all (`class B { v: number = 5 }` /
// `class D extends B { }` answers `d.v === 0`), which is a separate
// pre-existing hole confirmed on the clean HEAD and recorded rather
// than locked in here at its wrong answer.
class Base {
  v = 5;
  read() {
    return this.v;
  }
}
class Derived extends Base {
  extra = 6;
  own() {
    return this.extra;
  }
}

// the shapes that already worked, kept as ground: full annotations,
// a literal return, a parameter return, and a void method
class Annotated {
  v: number = 4;
  read(): number {
    return this.v;
  }
  self(): Annotated {
    return this;
  }
  lit() {
    return 7;
  }
  fromParam(a: number) {
    return a + 1;
  }
  noReturn(): void {
    this.v = this.v + 1;
  }
}

function main(): void {
  const c = new Counter();
  console.log(c.bump().bump().value());

  const l = new Labelled();
  console.log(l.read(), l.readSized(), l.both());

  const m = new Mixed();
  console.log(m.raw(), m.scaled(), m.viaOther());

  const br = new Branching();
  console.log(br.pick(true), br.pick(false), br.ternary(true), br.ternary(false));

  const d = new Derived();
  console.log(new Base().read(), d.own());

  const a = new Annotated();
  a.noReturn();
  console.log(a.read(), a.self().v, a.lit(), a.fromParam(41));
}

main();
