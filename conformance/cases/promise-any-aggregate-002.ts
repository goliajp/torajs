// An empty iterable settles a combinator SYNCHRONOUSLY — the tick,
// not just the answer.
//
// §27.2.4.1 step 8 / §27.2.4.2 step 8 reach remainingElementsCount 0
// before the call returns, so there is no element job to wait for and
// the result is already settled when `.then` attaches. tr routed every
// combinator result through one deferred settle, which is right for a
// non-empty input (the spec settles those through a round of element
// jobs, and minting them settled put the callbacks a microtask EARLY)
// and the mirror error for an empty one: a microtask LATE.
//
// The counter chain is the whole point of the file. This case used to
// be here alone WITHOUT it, because the divergence was still open and
// any interleaving would have encoded it; `Promise.any([])`'s answer
// was all that could be tested. Now the interleaving is the test.
//
// race is the one with nothing to settle: §27.2.4.5 step 3 leaves an
// empty race forever pending, so its handler never runs at all.

Promise.all([]).then((v: any) => {
  console.log("all", JSON.stringify(v));
});

Promise.allSettled([]).then((v: any) => {
  console.log("allSettled", JSON.stringify(v));
});

Promise.any([]).then(
  (v) => {
    console.log("any-unreachable", v);
  },
  (e: any) => {
    console.log("any", e.name, JSON.stringify(e.errors), e instanceof AggregateError);
  }
);

Promise.race([]).then((v: any) => {
  console.log("race-unreachable", v);
});

// A non-empty combinator still settles a tick later — it has an
// element job to run first — so `all1` lands after t1 where the empty
// ones land before.
Promise.all([Promise.resolve(1)]).then((v: any) => {
  console.log("all1", JSON.stringify(v));
});

Promise.resolve(0)
  .then(() => {
    console.log("t1");
  })
  .then(() => {
    console.log("t2");
  })
  .then(() => {
    console.log("t3");
  })
  .then(() => {
    console.log("t4");
  });

console.log("sync-last");
