// An empty iterable rejects Promise.any with an AggregateError over
// an empty list (§27.2.4.2 step 8 — remainingElementsCount reaches 0
// before the call returns, so there is no element that could ever
// fulfil it).
//
// This is its own case file rather than a case in -001 because of a
// divergence it would otherwise encode: bun settles an empty-input
// combinator SYNCHRONOUSLY, so its reaction runs one microtask ahead
// of every other combinator's, while tr routes all of them through
// the same deferred settle. Probed on Promise.all / .allSettled too
// and the gap is the same there, so it predates this and belongs to
// the whole family, not to any. Alone in a file there is nothing to
// interleave with and only the answer is under test.

Promise.any([]).then(
  (v) => {
    console.log("unreachable", v);
  },
  (e: any) => {
    console.log("empty", e.name, JSON.stringify(e.errors), e instanceof AggregateError);
  }
);

console.log("sync-last");
