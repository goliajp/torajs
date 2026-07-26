// `p.then(() => { … })` — the everyday side-effect handler, which
// declares no parameter. ES §27.2.5.4 hands the settled value to
// onFulfilled as one argument; how many the handler DECLARES is its
// own business, and the kernels call through `int64_t (*)(int64_t)`
// either way, so a 0-arg handler simply ignores its argument slot.
//
// The `Promise<Undefined>` receiver has admitted this since P10.2-A1.1
// and documents exactly that reasoning. Every typed receiver still
// refused it — "expected Function([Number], Number), got
// Function([], Void)" — for a shape bun runs.

const pn: Promise<number> = Promise.resolve(1);
pn.then(() => {
  console.log("num-recv zero-arg");
});

const ps: Promise<string> = Promise.resolve("s");
ps.then(() => {
  console.log("str-recv zero-arg");
});

// the receiver that already worked, kept as the reference shape
const pu = Promise.resolve();
pu.then(() => {
  console.log("undef-recv zero-arg");
});

// declaring the parameter must go on working
const keep: Promise<number> = Promise.resolve(3);
keep.then((v: number) => {
  console.log("one-arg still", v);
});

// and a 0-arg handler's RETURN still feeds the next cell
const chained: Promise<number> = Promise.resolve(2);
chained.then(() => 7).then((v: number) => {
  console.log("chain-after-zero", v);
});

// the two-handler station had the mirror of the same over-narrowing:
// it fixed each handler's signature at `(T) => T`, so the ordinary
// side-effect pair needed a `return v` added purely to satisfy it.
// §27.2.5.4 makes a handler's return the next cell's fulfilment
// value — it is under no obligation to be another T.
const two: Promise<number> = Promise.resolve(1);
two.then(
  (v: number) => {
    console.log("two-ok-void", v);
  },
  (e: number) => {
    console.log("two-err-void", e);
  },
);

// the shape that already worked must still answer Promise<T>
const same: Promise<number> = Promise.resolve(2);
same
  .then(
    (v: number) => v + 1,
    (e: number) => e,
  )
  .then((v: number) => {
    console.log("two-same-T", v);
  });

// the rejection leg, and zero-arg handlers in both slots
const rej: Promise<string> = Promise.reject("boom");
rej.then(
  (v: string) => {
    console.log("never", v);
  },
  (e: string) => {
    console.log("two-rejected", e);
  },
);

const zz: Promise<number> = Promise.resolve(5);
zz.then(
  () => {
    console.log("two-zero-arg");
  },
  () => {
    console.log("never2");
  },
);
