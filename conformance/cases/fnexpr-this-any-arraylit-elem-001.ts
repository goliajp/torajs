// 397-01 — an element of an array literal initializing an `: any`
// binding is a receiver-safe use of a this-reading fn-expr: the whole
// array lives in the any world, so `arr[0](7)` rides the any-index
// call lane (§13.3.6.2 — `this` is the array), a detached read's
// plain call answers undefined, and `.call` binds explicitly.

const fn = function () {
  return typeof this;
};
const add = function (n: number) {
  return typeof this + ":" + n;
};
const arr: any = [fn, add];
console.log(arr[0](7));
console.log(arr[1](5));
const g2 = arr[0];
console.log(g2());
console.log(arr[0].call({ tag: 1 }));
