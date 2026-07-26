// P-SURF S2.1 — generator methods as class members, `class C { *g() {} }`
// and `static *g() {}`. Unlike the object-literal position this is not
// pure parser work: the method is hoisted to a top-level `function*`
// taking the receiver as a parameter, because the generator desugar has
// no env channel and reaches its own state through `this`.

class Counter {
  n: number = 1;

  // the plain shape
  *plain() {
    yield 1;
    yield 2;
  }

  // the receiver has to survive across next() calls — it becomes a
  // field of the generated state-machine class
  *fromField() {
    yield this.n;
    yield this.n * 2;
  }

  // receiver and user params coexist, in that order
  *withArg(a: number) {
    yield a;
    yield this.n + a;
  }

  // an empty body is still a well-formed empty iterator
  *empty() {}

  // `*` inside the body is still multiplication
  *product() {
    yield 3 * 4;
  }

  // a static generator: `static` must accept `*` as what follows it
  static *stat() {
    yield 100;
    yield 200;
  }

  // an ordinary method next door keeps working
  plainMethod(): number {
    return this.n;
  }
}

const a = new Counter();
const b = new Counter();
b.n = 10;

console.log([...a.plain()]);
console.log([...a.empty()]);
console.log([...a.product()]);
console.log([...Counter.stat()]);
console.log(a.plainMethod(), b.plainMethod());

// per-instance state: two receivers, two answers, and re-reading one of
// them again afterwards still gives its own
console.log([...a.fromField()], [...b.fromField()], [...a.fromField()]);
console.log([...a.withArg(5)], [...b.withArg(5)]);

// each call mints a fresh iterator rather than sharing one
const i1 = a.plain();
const i2 = a.plain();
console.log(i1.next().value, i2.next().value, i1.next().value);

// stepping by hand reports `done` per the protocol
const it = b.fromField();
console.log(it.next().value, it.next().value, it.next().done);

// iterable in a for-of, and an early break does not throw
const seen: number[] = [];
for (const v of b.fromField()) {
  if (v === 20) break;
  seen.push(v);
}
console.log(seen);

// a subclass inherits the generator method and may add its own; both
// see the right receiver
class Sub extends Counter {
  *extra() {
    yield this.n + 5;
  }
}
const s = new Sub();
s.n = 3;
console.log([...s.fromField()], [...s.extra()]);

// a generator method reads enclosing module scope too
const scale = 1000;
class Scaled {
  *vals() {
    yield scale;
    yield scale + 1;
  }
}
console.log([...new Scaled().vals()]);
