// Promise.allKeyed / allSettledKeyed (await-dictionary proposal).
// bun 1.3.14 does not implement the proposal, so the oracle lives in
// the sibling .expected file (spec semantics: null-prototype result,
// §10.1.11.1 key order, per-key {status, value|reason} records).
//
// REJECTION faces (TypeError on non-object, first-rejection-wins,
// reason identity) are deliberately NOT in this fixture: a rejected
// combinator promise trips a PRE-EXISTING promise-pool double-drop
// (rc underflow -> cell recycle under the microtask queue; repros
// with plain `Promise.all(5 as any);` on the pre-knife HEAD, zero
// knife-3 code in the path). Recorded in plan-state as the next
// knife; the reject faces live in probes until it lands.
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
var allOk: any = {
  a: Promise.resolve(10),
  b: "plain",
};
Promise.allSettledKeyed(allOk).then(function (r: any) {
  console.log(r.a.status, r.a.value);
  console.log(r.b.status, r.b.value);
});
