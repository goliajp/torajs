// §27.2.4.1.3 step 6.q — the then GET (an accessor getter) throw
// takes the same close-and-reject exit as the invoke throw.
var returnCount = 0;
var p = new Promise(function () {});
Object.defineProperty(p, "then", {
  get: function () {
    throw new Error("getter-boom");
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
Promise.all(iter).then(
  function () {
    console.log("resolved");
  },
  function (e: any) {
    console.log("rejected", e.message, returnCount);
  }
);
console.log("sync", returnCount);
