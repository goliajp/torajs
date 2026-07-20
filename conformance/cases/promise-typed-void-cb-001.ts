// Typed `(v: T) => void` handlers over Promise<T> — TS callback-return
// variance: a side-effect handler is assignable where `(v: T) => T` is
// expected (bun runs all of these). Checker-only face: the
// heterogeneous arm admits Void alongside Number/String/Boolean rets;
// the kernel's REPR_VOID ret stamp (RFC 20260720-promise-any-cb
// knife 1) already zeroes the result leg. Matrix: int / str / bool
// sources × then / catch, fn-expr + arrow. Combinator sources stay
// out (pre-existing microtask-order + unhandled-rejection faces, L3b).
Promise.resolve(42).then(function (v: number) {
  console.log("int", v);
});
Promise.resolve("hi").then(function (s: string) {
  console.log("str", s);
});
Promise.resolve(true).then(function (b: boolean) {
  console.log("bool", b);
});
Promise.reject(9).catch(function (e: number) {
  console.log("caught", e);
});
Promise.resolve(7).then((v: number) => {
  console.log("arrow", v);
});
