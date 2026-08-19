// §27.2.4.1.3 step 6.i over the dyn (iterator) combinator lane with
// a patched Promise.resolve: the resolve call runs PER ELEMENT inside
// the iteration, so a throwing patch on an infinite iterator rejects
// after ONE next() and closes the iterator (§27.2.4.1 step 8.a) —
// the collect-then-delegate shape would loop forever.
function makeIter() {
  let nextCount = 0;
  let returnCount = 0;
  const it: any = {
    [Symbol.iterator]() {
      return {
        next() {
          nextCount++;
          return { value: 7, done: false };
        },
        return() {
          returnCount++;
          return { done: true, value: undefined };
        },
      };
    },
  };
  return { it, counts: () => "next=" + nextCount + " return=" + returnCount };
}

(Promise as any).resolve = function () {
  throw new Error("poisoned");
};

async function run(name: string, fn: (v: any) => Promise<any>) {
  const { it, counts } = makeIter();
  try {
    await fn(it);
    console.log(name, "fulfilled (wrong)");
  } catch (e: any) {
    console.log(name, "rejected", e.message, counts());
  }
}

async function main() {
  await run("all", (v) => Promise.all(v));
  await run("allSettled", (v) => Promise.allSettled(v) as any);
  await run("any", (v) => Promise.any(v));
  await run("race", (v) => Promise.race(v));
}
main();
