// proposal-array-from-async §2.1.1 step 3.j.ii.6.b — a sync mapfn
// throw on an infinite iterable closes the iterator exactly once
// and rejects with the thrown error. (The close tick itself is not
// probed — tr's sync-source MVP drives the loop synchronously.)
var closed = 0;
var n = 0;
var iterator: any = {
  next: function () {
    n += 1;
    return { value: 1, done: false };
  },
  return: function () {
    closed += 1;
    return { done: true };
  },
};
iterator[Symbol.iterator] = function () {
  return iterator;
};
(Array as any)
  .fromAsync(iterator, function (val: any) {
    throw new Error("mapfn-boom");
  })
  .then(
    function () {
      console.log("resolved");
    },
    function (e: any) {
      console.log("rejected", e.message, closed, n);
    }
  );
console.log("sync");
