// Two families the combinators answer correctly, neither of which had
// a fixture — and both sit on the paths rotation 696 rewrote (every
// element through `promiseResolve`, and the result through its
// resolve function). A rewrite that broke either would have been
// invisible.
//
// 1. IfAbruptRejectPromise. §27.2.4.1 step 3 and siblings: when
//    GetIterator is abrupt the combinator REJECTS its promise with
//    what was thrown. It does not throw synchronously out of the
//    call, and it does not resolve with an empty result.
function poisonedIterable(): any {
  const a: any = [];
  Object.defineProperty(a, Symbol.iterator, {
    get() { throw new Error("iter"); },
  });
  return a;
}

Promise.all(poisonedIterable()).then(
  () => console.log("all", "WRONG: fulfilled"),
  (e: any) => console.log("all", (e as Error).message),
);
Promise.allSettled(poisonedIterable()).then(
  () => console.log("allSettled", "WRONG: fulfilled"),
  (e: any) => console.log("allSettled", (e as Error).message),
);
Promise.any(poisonedIterable()).then(
  () => console.log("any", "WRONG: fulfilled"),
  (e: any) => console.log("any", (e as Error).message),
);
Promise.race(poisonedIterable()).then(
  () => console.log("race", "WRONG: fulfilled"),
  (e: any) => console.log("race", (e as Error).message),
);

// ...and it rejects RIGHT THERE, not a microtask later. The deferral
// the normal path takes exists because the spec settles a non-empty
// combinator through a round of element jobs; an abrupt GetIterator
// runs no jobs. This ordering is the whole assertion — the values
// above were already right while the handler ran one tick late.
const order: string[] = [];
Promise.all(poisonedIterable()).then(() => {}, () => { order.push("abrupt"); });
Promise.resolve(0)
  .then(() => { order.push("t1"); })
  .then(() => { order.push("t2"); })
  .then(() => console.log("order", order.join(",")));

// a non-iterable argument rejects with a TypeError rather than throwing
Promise.all(5 as any).then(
  () => console.log("non-iterable", "WRONG: fulfilled"),
  (e: any) => console.log("non-iterable", (e as Error).constructor.name),
);

// an abrupt `next()` rejects too, and the reason is what it threw
Promise.all({ [Symbol.iterator]() { return { next() { throw new Error("nx"); } }; } } as any)
  .then(
    () => console.log("next-throws", "WRONG: fulfilled"),
    (e: any) => console.log("next-throws", (e as Error).message),
  );

// 2. An element that IS a native promise but carries its OWN `then`.
//    §27.2.4.1.2 step 5.o invokes `then` on whatever promiseResolve
//    answered, and for a builtin-constructor promise that is the cell
//    itself — so the own property is what runs. The element rewrite
//    had to keep passing such a cell straight through rather than
//    wrapping it, or this answers 1 instead of "OWN".
function ownThen(): any {
  const p: any = Promise.resolve(1);
  p.then = function (res: any) { res("OWN"); };
  return p;
}

Promise.all([ownThen()]).then((v: any) => console.log("own-all", v[0]));
Promise.race([ownThen()]).then((v: any) => console.log("own-race", v));
Promise.allSettled([ownThen()]).then(
  (r: any) => console.log("own-allSettled", r[0].status, r[0].value),
);
