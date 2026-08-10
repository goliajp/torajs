// §27.2.4.5 through the interleaved lane: a then INVOKE throw on an
// infinite iterable closes the iterator once and rejects.
var returnCount = 0;
var p = new Promise(function () {});
Object.defineProperty(p, "then", {
  value: function () {
    throw new Error("race-boom");
  },
});
var iter: any = {};
iter[Symbol.iterator] = function () {
  return {
    next: function () {
      return { done: false, value: p };
    },
    return: function () {
      returnCount += 1;
      return {};
    },
  };
};
Promise.race(iter).then(
  function () {
    console.log("resolved");
  },
  function (e: any) {
    console.log("rejected", e.message, returnCount);
  }
);
console.log("sync", returnCount);
