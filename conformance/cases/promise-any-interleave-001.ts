// §27.2.4.2 through the interleaved lane: invoke throw closes once;
// and the fulfilment resolver short-circuits the outer.
var returnCount = 0;
var p = new Promise(function () {});
Object.defineProperty(p, "then", {
  value: function () {
    throw new Error("any-boom");
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
Promise.any(iter).then(
  function () {
    console.log("resolved");
  },
  function (e: any) {
    console.log("rejected", e.message, returnCount);
  }
);
var q = new Promise(function () {});
var qCalled = 0;
Object.defineProperty(q, "then", {
  value: function (onOk: any) {
    qCalled += 1;
    onOk(77);
  },
});
var n2 = 0;
var iter2: any = {};
iter2[Symbol.iterator] = function () {
  return {
    next: function () {
      n2 += 1;
      if (n2 <= 1) {
        return { done: false, value: q };
      }
      return { done: true, value: undefined };
    },
  };
};
Promise.any(iter2).then(function (v: any) {
  console.log("any-won", v, qCalled);
});
console.log("sync", returnCount);
