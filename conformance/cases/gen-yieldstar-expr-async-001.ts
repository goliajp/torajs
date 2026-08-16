// Expression-position `yield*` in async generators (§27.5.3.2 with
// generatorKind=async): done value from an inner async generator, and
// from a hand-rolled @@asyncIterator object answering a done step with
// a value — the exact shape the test262 yield-star-* family drives.
// Class-method and object-literal async generator hosts included.
async function* inner() {
  yield 3;
  return 9;
}

class C {
  async *m() {
    const v = yield* inner();
    console.log("class-done", v);
  }
}

const o = {
  async *m() {
    const v = yield* {
      [Symbol.asyncIterator]() {
        let n = 0;
        return {
          next() {
            n++;
            return n <= 2
              ? Promise.resolve({ value: n * 10, done: false })
              : Promise.resolve({ value: 42, done: true });
          },
        };
      },
    };
    console.log("objlit-done", v);
  },
};

async function main() {
  for await (const x of new C().m()) console.log(x);
  for await (const x of o.m()) console.log(x);
}
main();
