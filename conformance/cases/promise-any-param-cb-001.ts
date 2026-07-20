// RFC 20260720-promise-any-cb — `(v: any) => R` handlers over typed
// Promise<T> sources: checker admit (knife 2) + kernel repr-stamp
// boxing (knife 1) + the result-stake fix (a discarding `p.then(cb);`
// statement is exactly this file's shape on every line). Matrix:
// int / str / bool sources × then / catch × Void / Number / chained
// returns, fn-expr + arrow. Combinator sources stay out of the
// fixture: the sync combinators settle one microtask earlier than
// bun's absorption ordering and false-positive unhandled-rejection
// on absorbed REJECTED inputs — both pre-existing independent faces,
// L3b (value correctness of `Promise.any(...)` + any-cb is probe-
// verified in the RFC).
Promise.resolve(42).then(function (v: any) {
  console.log("int", v);
});
Promise.resolve("hi").then(function (v: any) {
  console.log("str", v);
});
Promise.resolve(true).then(function (v: any) {
  console.log("bool", v);
});
Promise.reject(9).catch(function (e: any) {
  console.log("caught", e);
});
Promise.resolve(5)
  .then(function (v: any) {
    return 10;
  })
  .then(function (w: any) {
    console.log("ret", w);
  });
Promise.resolve(7).then((v: any) => {
  console.log("arrow", v);
});
