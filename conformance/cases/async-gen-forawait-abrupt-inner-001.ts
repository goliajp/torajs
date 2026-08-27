// An async generator that `for await`s another async generator which
// completes abruptly, then is resumed. The output was already right —
// the process died afterwards, in the cycle collector's exit drain,
// on a cell whose class tag named the wrong class: the factory
// default-initializes the typed iterator slot, and that nested cell
// was stamped with the ENCLOSING class, so a three-field object wore
// a six-field layout and `child_offsets` walked past its allocation.
let err = new Error("boom");

async function* readFile() {
  yield Promise.reject(err);
  yield "unreachable";
}

async function* gen() {
  for await (const line of readFile()) {
    yield line;
  }
}

const it = gen();
it.next().then(
  () => console.log("resolved (wrong)"),
  (e) => {
    console.log("rejected", e.message);
    // The iterator is closed now; resuming is what used to leave the
    // collector a candidate to walk at exit.
    it.next().then((r) => {
      console.log("after", r.done, r.value);
      // Same shape, inner throwing instead of yielding a rejected
      // promise — one root, two spellings. Chained so the two
      // sequences interleave the same way tr and bun both order them.
      const it2 = outer();
      it2.next().then(
        () => console.log("resolved (wrong)"),
        (e2) => {
          console.log("threw", e2.message);
          it2.next().then((r2) => console.log("after2", r2.done, r2.value));
        },
      );
    });
  },
);

async function* thrower() {
  throw new Error("thrown");
}
async function* outer() {
  for await (const v of thrower()) {
    yield v;
  }
}

