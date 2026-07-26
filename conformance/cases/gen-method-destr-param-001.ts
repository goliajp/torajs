// P-SURF S2.8 — destructured parameters on a generator method, in both
// positions S2.1 opened. `*g({a, b}) {}` turns the parameter into a
// synthetic `__param_destr_N` plus a prefix of unpacking `let`s, and
// `desugar_generators` has to peel exactly those into the `__Gen_*`
// constructor. It learns how many from `ast.gen_param_destr_prefix`;
// both new paths now register there, as the decl form already did.

// object literal
const objlit = {
  *pair({ a, b }: { a: number; b: number }) {
    yield a;
    yield b;
  },
  *nested({ o }: { o: { n: number } }) {
    yield o.n;
    yield o.n * 2;
  },
  *mixed(lead: number, { a }: { a: number }) {
    yield lead;
    yield a;
  },
};
console.log([...objlit.pair({ a: 1, b: 2 })]);
console.log([...objlit.nested({ o: { n: 5 } })]);
console.log([...objlit.mixed(9, { a: 8 })]);

// class member — the receiver parameter is prepended ahead of the
// destructured one, and the peel count counts body statements, so the
// two do not interfere
class Holder {
  base: number = 100;

  *pair({ a, b }: { a: number; b: number }) {
    yield a;
    yield b;
  }

  // destructured param alongside a receiver field read
  *withField({ a }: { a: number }) {
    yield this.base;
    yield this.base + a;
  }

  // two destructured params
  *two({ a }: { a: number }, { b }: { b: number }) {
    yield a;
    yield b;
    yield this.base;
  }

  static *stat({ a }: { a: number }) {
    yield a;
    yield a + 1;
  }
}

const h = new Holder();
console.log([...h.pair({ a: 3, b: 4 })]);
console.log([...h.withField({ a: 7 })]);
console.log([...h.two({ a: 1 }, { b: 2 })]);
console.log([...Holder.stat({ a: 20 })]);

// per-instance state still holds with a destructured param in play
const h2 = new Holder();
h2.base = 500;
console.log([...h.withField({ a: 1 })], [...h2.withField({ a: 1 })]);

// each call gets its own unpacked values rather than sharing the field
const i1 = h.pair({ a: 11, b: 12 });
const i2 = h.pair({ a: 21, b: 22 });
console.log(i1.next().value, i2.next().value, i1.next().value, i2.next().value);

// stepping by hand reports done, same as any generator
const it = h.pair({ a: 1, b: 2 });
console.log(it.next().value, it.next().value, it.next().done);

// the decl form this mirrors, as a control
function* declForm({ a, b }: { a: number; b: number }) {
  yield a;
  yield b;
}
console.log([...declForm({ a: 30, b: 31 })]);
