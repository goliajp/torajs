// §27.2.4.7.1 PromiseResolve(C, x) steps 1-2 — a native promise is
// returned as itself only when `x.constructor` is C. A promise whose
// `constructor` was reassigned, and a subclass instance handed to the
// builtin `Promise.resolve`, get a fresh promise that adopts them
// (test262 `Promise/resolve/arg-uniq-ctor`). Every native promise took
// the identity road before (rotation 650).
var p1: any = new Promise(function () {});
p1.constructor = null;
var p2: any = Promise.resolve(p1);
console.log("uniq", p1 === p2, p1.constructor === null);
// the fresh promise adopts p1 through `p1.then`, whose species lookup
// reads the null constructor and rejects — swallow that rejection so
// the oracle's exit is quiet (tr adopts by native attach: recorded)
p2.catch(function () {});
var p3: any = new Promise(function () {});
console.log("same", Promise.resolve(p3) === p3);
class S extends Promise<any> {}
var s: any = S.resolve(1);
console.log("sub", Promise.resolve(s) === s, S.resolve(s) === s);
var q: any = Promise.resolve(7);
q.constructor = Object;
Promise.resolve(q).then(function (v: any) { console.log("adopted", v); });
