// §27.2.4.1.3 — a patched `Promise.resolve` binds this = the
// constructor, on the direct static call and on every combinator's
// per-element invocation (the store-position receiver face).
Promise.resolve = function (v: any) {
  console.log("this-is-ctor", this === Promise);
  return new Promise(function (res: any) { res("t:" + v); });
};
var p = Promise.resolve(4);
p.then(function (x: any) { console.log("then", x); });
function mk(v: any): any { return new Promise(function (res: any) { res(v); }); }
var a = Promise.all([mk(1)]);
a.then(function (xs: any) { console.log("all", xs[0]); });
console.log("sync");
