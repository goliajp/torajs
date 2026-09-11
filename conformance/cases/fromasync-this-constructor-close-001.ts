// proposal-array-from-async §2.1.1 step 3.j.ii.11 — under a
// constructor `this`, an element is defined AS IT SETTLES, and a
// define the product refuses (a non-configurable index the
// constructor pre-planted) closes the iterator under the pending
// TypeError. The collect-then-construct shape this replaces never
// reached the define on an infinite iterator — it collected forever.
function runTest(fn: any): void {
  fn();
}
runTest(async function () {
  function Pinned(this: any) {
    Object.defineProperty(this, 0, {
      enumerable: true,
      writable: true,
      configurable: false,
      value: 0,
    });
  }
  let closedSync = false;
  const forever: any = {
    next(): any {
      return { value: 1, done: false };
    },
    return(): any {
      closedSync = true;
      return { done: true };
    },
    [Symbol.iterator](): any {
      return this;
    },
  };
  try {
    await Array.fromAsync.call(Pinned, forever);
    console.log("BAD: fulfilled");
  } catch (e) {
    console.log("sync:", e instanceof TypeError, closedSync);
  }

  let closedAsync = false;
  const foreverAsync: any = {
    next(): any {
      return Promise.resolve({ value: 1, done: false });
    },
    return(): any {
      closedAsync = true;
      return Promise.resolve({ done: true });
    },
    [Symbol.asyncIterator](): any {
      return this;
    },
  };
  try {
    await Array.fromAsync.call(Pinned, foreverAsync);
    console.log("BAD: fulfilled");
  } catch (e) {
    console.log("async:", e instanceof TypeError, closedAsync);
  }

  // Construct runs before the first step, and the product takes the
  // elements as they come — a plain finite source still finishes.
  let order: string[] = [];
  function Log(this: any) {
    order.push("ctor");
  }
  const src: any = {
    [Symbol.iterator](): any {
      let i = 0;
      return {
        next(): any {
          order.push("next");
          return i < 2 ? { value: i++, done: false } : { value: undefined, done: true };
        },
      };
    },
  };
  const a: any = await Array.fromAsync.call(Log, src);
  console.log(order.join(","), a.length, a[0], a[1]);
  console.log("done");
});
