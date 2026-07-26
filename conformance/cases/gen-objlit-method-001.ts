// P-SURF S2.1 — generator method shorthand in an object literal,
// `{ *g() { yield 1 } }` (ES §13.2.5 sugar for `{ g: function*() {} }`).
// The generator substrate was already whole; only this parse position
// refused, with `expected field name in object literal, got Star`.

// the plain shape
const basic = {
  *g() {
    yield 1;
    yield 2;
  },
};
console.log([...basic.g()]);

// an empty body still yields a well-formed empty iterator, and a
// generator method sitting beside ordinary fields does not disturb them
const mixed = {
  before: 10,
  *empty() {},
  after: 20,
};
console.log([...mixed.empty()], mixed.before, mixed.after);

// parameters, including defaults, reach the body
const withParams = {
  *pair(a: number, b: number) {
    yield a;
    yield b;
  },
  *dflt(n = 5) {
    yield n;
  },
};
console.log([...withParams.pair(7, 8)]);
console.log([...withParams.dflt()], [...withParams.dflt(9)]);

// two generator methods on one object get independent state, and each
// call mints a fresh iterator rather than sharing one
const twice = {
  *a() {
    yield 1;
  },
  *b() {
    yield 2;
  },
};
console.log([...twice.a()], [...twice.b()], [...twice.a()]);

// stepping by hand: the method is a real generator, so `next()`
// reports `done` the way the protocol requires
const stepped = {
  *two() {
    yield "x";
    yield "y";
  },
};
const it = stepped.two();
console.log(it.next().value, it.next().value, it.next().done);

// a generator method is iterable in a for-of, and closing early
// (`break`) does not throw
const looped = {
  *nums() {
    yield 1;
    yield 2;
    yield 3;
  },
};
const seen: number[] = [];
for (const n of looped.nums()) {
  if (n === 3) break;
  seen.push(n);
}
console.log(seen);

// the body sees enclosing scope, and a return annotation on the method
// is accepted (it collapses to the yield type the desugar consumes)
const outer = 100;
const annotated = {
  *vals(): Generator<number> {
    yield outer;
    yield outer + 1;
  },
};
console.log([...annotated.vals()]);

// `*` elsewhere is still multiplication — the lookahead is `* Ident (`,
// nothing broader
const product = 3 * 4;
console.log(product, { m: product * 2 }.m);
