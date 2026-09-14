// §27.2.4.1.3 step 6.q-s (and the race / any / allSettled mirrors):
// a combinator must Get each element's `then` and Call it with a
// fresh resolveElement / rejectElement pair. The sync kernels the
// typed tier hands an array literal to walked the cells' state
// directly, so an element wearing its own `then` was bypassed:
// `cnt` stayed 0 and a throwing override never rejected the outer
// (rotation 651). One override now sends the whole input down the
// iterator-interleaved lane, which already implements the protocol.
var p1 = new Promise(function () {});
var p2 = new Promise(function () {});
var p3 = new Promise(function () {});
var cnt = 0;
var seen: any[] = [];
p1.then = p2.then = p3.then = function (a: any, b: any) {
  cnt++;
  seen.push([typeof a, typeof b, a.length, b.length, arguments.length, this === p1, this === p2, this === p3]);
};
Promise.all([p1, p2, p3]);
console.log("all", cnt, JSON.stringify(seen));

var pr = new Promise(function () {});
var error = new Error("boom");
Object.defineProperty(pr, "then", { value: function () { throw error; } });
Promise.all([pr]).then(function () { console.log("all fulfilled?!"); }, function (reason) { console.log("all rejected", reason === error); });
Promise.race([pr]).then(function () { console.log("race fulfilled?!"); }, function (reason) { console.log("race rejected", reason === error); });
Promise.allSettled([pr]).then(function () { console.log("allSettled fulfilled?!"); }, function (reason) { console.log("allSettled rejected", reason === error); });
Promise.any([pr]).then(function () { console.log("any fulfilled?!"); }, function (reason) { console.log("any rejected", reason === error); });

// The override settles through the pair it was handed: the outer
// still resolves.
let q: any = Promise.resolve(7);
let qcnt = 0;
q.then = function (r: any, j: any) { qcnt++; r(7); };
Promise.any([q]).then((v: any) => console.log("any settled", v, qcnt));
Promise.all([q]).then((v: any) => console.log("all settled", JSON.stringify(v), qcnt));
Promise.race([q]).then((v: any) => console.log("race settled", v, qcnt));
Promise.allSettled([q]).then((v: any) => console.log("allSettled settled", JSON.stringify(v), qcnt));

// No override anywhere: the plain walk is untouched.
Promise.all([Promise.resolve(1), Promise.resolve(2)]).then((v) => console.log("plain", JSON.stringify(v)));
