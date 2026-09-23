// §27.2.4.1 step 3 runs GetIterator on the argument, and step 4's
// IfAbruptRejectPromise turns an abrupt there into a REJECTED promise.
// An Array's @@iterator is observable — an own accessor replaces it —
// so walking the backing store instead answers as if the array were
// empty: `all` / `allSettled` fulfilled, `race` stayed pending forever
// and `any` rejected with an empty AggregateError.

function poisoned(): any {
  const xs: any = [];
  Object.defineProperty(xs, Symbol.iterator, {
    get() { throw new Error("BOOM"); },
  });
  return xs;
}

Promise.all(poisoned()).then(
  () => console.log("all fulfilled"),
  (e: any) => console.log("all", e.message),
);
Promise.allSettled(poisoned()).then(
  () => console.log("allSettled fulfilled"),
  (e: any) => console.log("allSettled", e.message),
);
Promise.race(poisoned()).then(
  () => console.log("race fulfilled"),
  (e: any) => console.log("race", e.message),
);
Promise.any(poisoned()).then(
  () => console.log("any fulfilled"),
  (e: any) => console.log("any", e.message),
);

// An ordinary array still resolves through the same lane.
Promise.all([Promise.resolve(1), Promise.resolve(2)]).then((v: any) =>
  console.log("plain", v[0], v[1]),
);
