// §27.2.4.1.3 step 6.r + §27.2.4.1 step 8.a — a then INVOKE throw
// aborts the iteration (infinite iterable!) and closes the iterator
// exactly once; the outer rejects with the thrown error.
var returnCount = 0;
var nextCount = 0;
var p = new Promise(function () {});
Object.defineProperty(p, "then", {
  value: function () {
    throw new Error("boom");
  },
});
var iter: any = {};
iter[Symbol.iterator] = function () {
  return {
    next: function () {
      nextCount += 1;
      return { done: false, value: p };
    },
    return: function () {
      returnCount += 1;
      return {};
    },
  };
};
Promise.all(iter).then(
  function () {
    console.log("resolved");
  },
  function (e: any) {
    console.log("rejected", e.message, returnCount, nextCount);
  }
);
console.log("sync", returnCount);
