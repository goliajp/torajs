// §27.2.4.3 through the interleaved lane: the then GET (accessor)
// throw closes the iterator once and rejects the outer.
var returnCount = 0;
var p = new Promise(function () {});
Object.defineProperty(p, "then", {
  get: function () {
    throw new Error("settled-boom");
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
Promise.allSettled(iter).then(
  function () {
    console.log("resolved");
  },
  function (e: any) {
    console.log("rejected", e.message, returnCount);
  }
);
console.log("sync", returnCount);
