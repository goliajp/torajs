// cluster #1 blade 2 — spec §23.1.3 higher-order callbacks receive
// (elem, index, sourceArray). The checker formals declare the full
// arity (srcArray as the kind-aware Array<Any> view), the closure
// param inference seeds (elem, number, any[]) positions, and the
// lowering appends the actual index / source-array per the callback's
// own declared arity.
var arr = [10, 20, 30];
console.log(arr.every(function (val, idx, obj) { return obj[idx] === val; }));
console.log(arr.map(function (v, i) { return v + i; }));
console.log(arr.filter(function (v, i, a) { return i > 0 && a.length === 3; }));
var idxs: number[] = [];
arr.forEach(function (v, i) { idxs.push(i); });
console.log(idxs);
console.log(arr.reduce(function (acc, cur, i, a) { return acc + cur + i + a.length; }, 0));
console.log(arr.findIndex(function (v, i) { return i === 2; }));
console.log(arr.some(function (v, i, a) { return v === a[a.length - 1]; }));
console.log(arr.find(function (v, i) { return i === 1; }));

// a this-using named fn as the callback value rides the forwarder,
// whose public face skips the promoted `__this` param and feeds
// `undefined` into the target (obj[idx] with any-typed idx is the
// any-keyed-index boundary, L3b — kept off this fixture)
function callbackfn(val, idx, obj) {
  return this === undefined && val > 5;
}
console.log(arr.every(callbackfn));

// arrow shapes take the same position seeds
console.log(arr.map((v, i) => v * 10 + i));
console.log(arr.filter((v, i, a) => a[i] === v));
