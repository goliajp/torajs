// proposal-upsert `Map.prototype.getOrInsertComputed` /
// `WeakMap.prototype.getOrInsertComputed` compute the missing value
// with `Call(callbackfn, undefined, «key»)` — one argument, and no
// thisArg anywhere in the signature. So a function EXPRESSION in that
// slot reads `this` as undefined; tr refused to compile it, because a
// `this` no receiver-promoting knife claims is left as an unbound
// `__this` capture.
//
// The receiver has to be a certain Map / WeakMap, which is the same
// bar the promise handlers hold: a user object with a method of that
// name decides for itself how it calls what it is handed.

const m: any = new Map();

const computed = m.getOrInsertComputed(1, function (k: any) {
  return "computed:" + k + ":" + typeof this;
});
console.log(computed, m.get(1));

// present key — the callback does not run at all
const present = m.getOrInsertComputed(1, function (k: any) {
  return "second-call";
});
console.log(present);

// written in place, without the binding
console.log(
  new Map().getOrInsertComputed("k", function (k: any) {
    return typeof this;
  }),
);

const wm: any = new WeakMap();
const key: any = {};
console.log(
  wm.getOrInsertComputed(key, function (k: any) {
    return this === undefined ? "no-receiver" : "receiver";
  }),
);
