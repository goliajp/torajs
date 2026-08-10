// §27.2 then override via accessor: the GET runs the getter, its
// answer is invoked, and a getter throw is a catchable abrupt.
var p = new Promise(function () {});
Object.defineProperty(p, "then", {
  get: function () {
    console.log("getter");
    return function () {
      console.log("user then");
      return 2;
    };
  },
});
(p as any).then(function () {});
var q = new Promise(function () {});
Object.defineProperty(q, "then", {
  get: function () {
    throw new Error("boom");
  },
});
try {
  (q as any).then(function () {});
} catch (e: any) {
  console.log("caught", e.message);
}
console.log("after");
