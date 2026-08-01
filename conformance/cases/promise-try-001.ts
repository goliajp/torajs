// ES2025 Promise.try (§27.2.4.8) — desugars to an immediately-
// invoked try/catch arrow over the resolve/reject statics: the
// callback runs SYNCHRONOUSLY, its return value resolves, a
// synchronous throw becomes a rejection, extra args forward.
// (Thenable-flattening of an any-typed return rides the pre-existing
// Promise.resolve adopt gap — L3b, not covered here.)

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

console.log("main-end");
