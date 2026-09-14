// proposal-array-from-async §2.1.1 — the observable get order on an array-like
// source (@@asyncIterator, @@iterator, length, 0, 1); an array source is iterated
// live, so a push / pop after the call is seen; an async iterable's element promise
// is NOT awaited (only the `next()` result is).
async function main(): Promise<void> {
  const actual: string[] = [];
  const items: any = {};
  function observe(obj: any, key: any, value: any, name: string): void {
    Object.defineProperty(obj, key, {
      get() {
        actual.push("get " + name);
        return value;
      },
      configurable: true,
    });
  }
  observe(items, Symbol.asyncIterator, undefined, "asyncIterator");
  observe(items, Symbol.iterator, undefined, "iterator");
  observe(items, "length", 2, "length");
  observe(items, 0, 2, "0");
  observe(items, 1, 1, "1");
  const r = await Array.fromAsync(items);
  console.log(r.join(","));
  console.log(actual.join("|"));
  const arr = [1, 2, 3];
  const p = Array.fromAsync(arr);
  arr.push(4);
  console.log((await p).join(","));
  const arr2 = [1, 2, 3];
  const p2 = Array.fromAsync(arr2);
  arr2.pop();
  console.log((await p2).join(","));
  const prom = Promise.resolve({});
  const ai: any = {
    [Symbol.asyncIterator]() {
      let i = 0;
      return {
        next() {
          if (i > 0) {
            return Promise.resolve({ done: true });
          }
          i++;
          return Promise.resolve({ value: prom, done: false });
        },
      };
    },
  };
  const r3 = await Array.fromAsync(ai);
  console.log(r3.length, r3[0] === prom);
  console.log("done");
}
main();
