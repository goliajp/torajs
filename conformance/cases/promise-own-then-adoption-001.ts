// §27.2.1.3.2 step 8 — the resolve function reads `then` off EVERY
// object, so a native promise carrying an OWN `then` resolves through
// that override's job, not through the adopt-by-attach fast path.
// §27.2.4.7 PromiseResolve is a different algorithm: it compares
// `constructor` and answers x itself, never reading `then`.

const value: any = { tag: "V" };

// The resolve function's resolution is a promise with an own `then`.
const t1: any = new Promise(function (r: any) { r(); });
t1.then = function (r: any) { r(value); };
new Promise(function (r: any) { r(t1); }).then((v: any) => console.log("1", v.tag));

// A handler's RETURN value takes the same road (§27.2.2.1 step 1.h).
const t2: any = new Promise(function (r: any) { r(); });
t2.then = function (r: any) { r(value); };
let go: any;
const p2: any = new Promise(function (r: any) { go = r; });
p2.then(function () { return t2; }).then((x: any) => console.log("2", x.tag));
go();

// step 10 captures thenAction AT resolve time — a later store is dead.
let late = 0;
const t3: any = new Promise(function (r: any) { r(); });
t3.then = function (r: any) { r(value); };
const p3 = new Promise(function (r: any) { r(t3); });
t3.then = function () { late += 1; };
p3.then((v: any) => console.log("3", v.tag, late));

// An abrupt override rejects the derived promise.
const t4: any = new Promise(function (r: any) { r(); });
t4.then = function () { throw new Error("BOOM"); };
new Promise(function (r: any) { r(t4); })
  .then(() => console.log("4 unreachable"), (e: any) => console.log("4", e.message));

// A `then` that is NOT callable falls back to the plain fulfill.
const t5: any = new Promise(function (r: any) { r("FIVE"); });
t5.then = 7;
new Promise(function (r: any) { r(t5); }).then((v: any) => console.log("5", v));
