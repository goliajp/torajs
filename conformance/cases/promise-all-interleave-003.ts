// §27.2.4.1.3 steps 6.o-r the fulfilment way: the user then override
// receives a real resolveElement / reject pair; calling the first
// parks the element value, and the outer resolves once the iteration
// ends and every element reported in.
var called = 0;
var p = new Promise(function () {});
Object.defineProperty(p, "then", {
  value: function (onOk: any, onErr: any) {
    called += 1;
    onOk(called * 10);
  },
});
var n = 0;
var iter: any = {};
iter[Symbol.iterator] = function () {
  return {
    next: function () {
      n += 1;
      if (n <= 2) {
        return { done: false, value: p };
      }
      return { done: true, value: undefined };
    },
  };
};
Promise.all(iter).then(function (vals: any) {
  console.log("resolved", vals.length, vals[0], vals[1], called);
});
console.log("sync", called);
