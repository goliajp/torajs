// Promise.allKeyed / allSettledKeyed (await-dictionary proposal).
// bun 1.3.14 does not implement the proposal, so the oracle lives in
// the sibling .expected file (spec semantics: null-prototype result,
// §10.1.11.1 key order, first-rejection-wins for allKeyed, per-key
// {status, value|reason} records for allSettledKeyed, TypeError
// REJECTION on a non-object argument).
var input: any = {
  first: Promise.resolve(1),
  second: 2,
  third: Promise.resolve("three"),
};
Promise.allKeyed(input).then(function (r: any) {
  console.log(Object.getPrototypeOf(r) === null);
  console.log(Object.keys(r));
  console.log(r.first, r.second, r.third);
});
Promise.allKeyed(5 as any).then(null, function (e: any) {
  console.log("rejected", e instanceof TypeError);
});
var resolveFirst: any;
var resolveSecond: any;
var pending: any = {
  first: new Promise(function (resolve: any) { resolveFirst = resolve; }),
  second: new Promise(function (resolve: any) { resolveSecond = resolve; }),
};
var combined: any = Promise.allKeyed(pending);
resolveSecond("second");
resolveFirst("first");
combined.then(function (r: any) {
  console.log(Object.keys(r), r.first, r.second);
});
Promise.allKeyed({} as any).then(function (r: any) {
  console.log("empty", Object.keys(r).length, Object.getPrototypeOf(r) === null);
});
var mixed: any = {
  ok: Promise.resolve(1),
  bad: Promise.reject(new Error("boom")),
};
Promise.allSettledKeyed(mixed).then(function (r: any) {
  console.log(r.ok.status, r.ok.value);
  console.log(r.bad.status, r.bad.reason instanceof Error);
});
Promise.allKeyed({ nope: Promise.reject(new TypeError("no")) } as any).then(null, function (e: any) {
  console.log("allKeyed rejected", e instanceof TypeError);
});
