// ES2025 Promise.try (§27.2.4.8) — desugars to an immediately-
// invoked try/catch arrow over the resolve/reject statics: the
// callback runs SYNCHRONOUSLY, its return value resolves, a
// synchronous throw becomes a rejection, extra args forward, and a
// returned promise flattens (the any-lane resolve kernel adopts a
// boxed %Promise% cell instead of double-wrapping it).

Promise.try(function () {
  return 42;
}).then(function (v) {
  console.log("resolved", v);
});

// args forward + synchronous execution order
Promise.try(function (a: any, b: any) {
  console.log("sync", a, b);
  return a + b;
}, 1, 2).then(function (v) {
  console.log("sum", v);
});

// throw -> rejection
Promise.try(function () {
  throw new Error("boom");
}).catch(function (e: any) {
  console.log("caught", e.message);
});

// promise return flattens through the adopt probe. NOTE: kept as
// the only promise-returning chain in this file — native
// Promise.try resolves through a resolve function (thenable job,
// +1 tick) while the desugar rides the Promise.resolve static
// (step-2 pass-through), so cross-chain interleaving differs
// (L3b ledger); within one chain the output is identical.
Promise.try(function () {
  return Promise.resolve("inner");
}).then(function (v) {
  console.log("flat", v);
});

console.log("main-end");
