// §27.2.4.1.3 step 6.i (and the race/any/allSettled siblings) —
// with a patched `Promise.resolve` every combinator invokes it per
// element with this = the constructor; a throwing override rejects
// the combinator's promise (IfAbruptRejectPromise).
var count = 0;
Promise.resolve = function (v: any) { count = count + 1; return v; };
function mk(v: any): any { return new Promise(function (res: any) { res(v); }); }

var p = Promise.all([mk(1), mk(2), mk(3)]);
p.then(function (xs: any) { console.log("all", xs[0], xs[1], xs[2], "count", count); });

var r = Promise.race([mk(10), mk(20)]);
r.then(function (x: any) { console.log("race", x, "count", count); });

var s = Promise.allSettled([mk(30)]);
s.then(function (rs: any) { console.log("settled", rs[0].status, rs[0].value, "count", count); });

async function af(v: number) { return v + 100; }
var t = Promise.all([af(7), af(8)]);
t.then(function (xs: any) { console.log("typed-all", xs[0], xs[1], "count", count); });

console.log("sync", count);
