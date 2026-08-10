// §27.2 promise instances are ordinary objects: a defineProperty
// "then" landing replaces the builtin for an any-lane call, and its
// return value is the call's value (no result promise minted).
var p = new Promise(function () {});
Object.defineProperty(p, "then", {
  value: function () {
    console.log("user then");
    return 1;
  },
});
console.log(typeof (p as any).then);
(p as any).then(
  function () {},
  function () {}
);
// A promise without an override keeps the builtin bridge.
var q = Promise.resolve(7);
(q as any).then(function (v: any) {
  console.log("builtin", v);
});
console.log("after");
