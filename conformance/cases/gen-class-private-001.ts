// P-SURF S2.1 × S2.2 — a generator method with a private name,
// `class C { *#g() {} }`.
//
// S2.1 taught the class-member position to accept `*`, but its member
// name only accepted `Ident` and the reserved words, so `*#priv()` fell
// through to `expected generator method name after *`. The private half
// was never the blocker: `#x` fields and `#m()` methods already work,
// and a private field read from inside an ordinary generator method
// already worked too. Only the generator method's own *name* was
// missing the case.
//
// It takes the same route the ordinary-member path does — mangle to
// `__priv_<Class>__<name>` and force Private — so the receiver forwarder,
// the vtable and `this.#g()` all resolve without a parallel data path.

class Counter {
  n: number = 2;

  // the plain shape
  *#steps() {
    yield 1;
    yield 2;
  }

  // reads the receiver across next() calls, through the hoisted
  // receiver parameter
  *#scaled() {
    yield this.n;
    yield this.n * 10;
  }

  // parameters sit behind the synthesized receiver, as for any
  // generator method
  *#plus(k: number) {
    yield this.n + k;
    yield this.n * k;
  }

  // a private generator calling a private ordinary method
  #base(): number {
    return this.n + 100;
  }
  *#viaMethod() {
    yield this.#base();
  }

  // a public generator next door, and one reaching a private *method*
  // — the private name has to be visible from sibling members either
  // way round
  *pub() {
    yield 0;
  }
  *mixed() {
    yield this.#base();
    yield 99;
  }

  // the readers print rather than return. Handing a generator spread
  // back across a function return answers garbage — three lines
  // reproduce it on a free `function*`, so it has nothing to do with
  // private names — and it is filed as S8.5.
  show(): void {
    console.log([...this.#steps()], [...this.#scaled()], [...this.#plus(3)]);
    console.log([...this.#viaMethod()], [...this.pub()], [...this.mixed()]);
  }
}

const c = new Counter();
c.show();

// per-instance state still holds with a private generator
const d = new Counter();
d.n = 5;
d.show();
c.show();

// stepping by hand through a private generator reached from a public
// method
class Steps {
  *#it() {
    yield "a";
    yield "b";
  }
  first(): string {
    const i = this.#it();
    return i.next().value + i.next().value + String(i.next().done);
  }
}
console.log(new Steps().first());

// a subclass declaring its own private generator of the same name —
// `#` is hard-private, so the two never collide
class Base {
  *#tag() {
    yield "base";
  }
  read(): void {
    console.log([...this.#tag()]);
  }
}
class Derived extends Base {
  *#tag() {
    yield "derived";
  }
  readOwn(): void {
    console.log([...this.#tag()]);
  }
}
const e = new Derived();
e.read();
e.readOwn();
