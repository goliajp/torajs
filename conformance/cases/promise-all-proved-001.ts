// Promise.all under both whole-program proofs (no object can get a
// `then`, Promise.resolve is never written) over an any-shape input:
// the elements bound to a Promise<T>[] come from an `any` value, so
// they ride NaN-box slots and each goes through PromiseResolve first.
// A promise answers itself, anything else a promise fulfilled with
// it, a plain object included; a pending element is waited on, and a
// rejected one rejects the result with its reason.
let fire: (v: number) => void = (_v: number) => {};
const pending = new Promise<number>((res) => {
  fire = res;
});
function mixed(): any {
  return [Promise.resolve(1), 2, "three", { four: 4 }, undefined, null];
}
function withPending(): any {
  return [Promise.resolve(10), pending, 30];
}
function withReject(): any {
  return [Promise.resolve(1), Promise.reject("boom"), 3];
}
function empty(): any {
  return [];
}
const a: Promise<number>[] = mixed();
const b: Promise<number>[] = withPending();
const c: Promise<number>[] = withReject();
const d: Promise<number>[] = empty();
Promise.all(a).then((v) => console.log("a", v.length, v[0], v[1], v[2], v[4], v[5]));
Promise.all(b).then((v) => console.log("b", v[0] + v[1] + v[2], v.length));
Promise.all(c).then(
  (v) => console.log("c fulfilled", v.length),
  (e) => console.log("c rejected", e),
);
Promise.all(d).then((v) => console.log("d", v.length));
fire(20);
