// An async generator that iterates another one with `for await` and
// yields through it — the typed lane, where the outer state machine
// carries the inner iterator as a declared class-typed field. The
// inner completes abruptly and the outer is resumed afterwards, which
// is what walks the inner cell's fields at exit.
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
async function* thrower() {
  throw new Error("thrown");
}
async function* outer() {
  for await (const v of thrower()) {
    yield v;
  }
}
const it = gen();
it.next().then(
  () => console.log("resolved (wrong)"),
  (e) => {
    console.log("rejected", e.message);
    it.next().then((r) => {
      console.log("after", r.done, r.value);
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
