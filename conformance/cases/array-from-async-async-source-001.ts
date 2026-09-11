// proposal-array-from-async §2.1.1 step 3.a — GetIterator runs with
// hint=async, so an async generator object and a hand-written
// `@@asyncIterator` are the sources fromAsync is FOR. Walking them
// with the sync step protocol instead read `done` off the Promise
// `next()` answers (never true) and spun forever.

async function main(): Promise<void> {
  async function* gen(): any {
    yield 0;
    yield 1;
  }
  const r1 = await Array.fromAsync(gen());
  console.log(r1.join(","));

  // mapfn over the same source — element k awaits, maps, awaits.
  const r2 = await Array.fromAsync(gen(), function (v: any, k: any): any {
    return v * 10 + k;
  });
  console.log(r2.join(","));

  // A hand-written async iterator: only `@@asyncIterator`, no
  // `@@iterator` and no `length`, so the array-like tail used to
  // answer the empty walk.
  const src: any = {
    [Symbol.asyncIterator](): any {
      let i = 0;
      return {
        next(): any {
          return Promise.resolve(i < 3 ? { value: i++, done: false } : { value: undefined, done: true });
        },
      };
    },
  };
  const r3 = await Array.fromAsync(src);
  console.log(r3.join(","));

  // A sync-only source still rides the Async-from-Sync value await.
  function* sync(): any {
    yield Promise.resolve("a");
    yield "b";
  }
  const r4 = await Array.fromAsync(sync());
  console.log(r4.join(","));

  // The array-like tail is still there for a source with no iterator.
  const like: any = { length: 2, 0: "x", 1: "y" };
  const r5 = await Array.fromAsync(like);
  console.log(r5.join(","));
  console.log("done");
}
main();
