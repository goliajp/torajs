// The TYPED `Promise.resolve(<Promise>)` arm passed the promise
// through unconditionally ("tr has no Promise subclassing"); §27.2.4.7.1
// step 2 returns x as itself only when `x.constructor` is C. A typed
// binding whose `constructor` expando was reassigned (the test262
// `arg-uniq-ctor` spelling) and a subclass instance now ride the
// any-lane kernel, which mints a fresh promise that adopts them
// (rotation 650). The plain case keeps its identity.
var p1 = new Promise(function () {});
(p1 as any).constructor = null;
var p2 = Promise.resolve(p1);
console.log("typed-uniq", p1 === p2);
p2.catch(function () {});
var p3 = new Promise<number>(function (r) { r(1); });
console.log("typed-same", Promise.resolve(p3) === p3);
class S extends Promise<number> {}
var s = S.resolve(2);
console.log("typed-sub", Promise.resolve(s) === s);
Promise.resolve(s).then(function (v) { console.log("adopted", v); });
