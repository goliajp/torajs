// §23.1.3.{12,15,16,17,23,25,29} step 3 — `Call(callbackfn, thisArg,
// «kValue, k, O»)`. On an array-LIKE receiver the callback's receiver
// slot was left at the buffer's `undefined`, so every one of these
// answered as if no thisArg had been passed — while the same call on a
// real array honoured it.
const obj: any = { 0: 11, 1: 22, length: 2 };
const thisArg: any = { tag: "T" };
function cb(this: any) { return this === thisArg; }

console.log((Array.prototype.every as any).call(obj, cb, thisArg));
console.log((Array.prototype.some as any).call(obj, cb, thisArg));
console.log((Array.prototype.filter as any).call(obj, cb, thisArg).length);
console.log((Array.prototype.map as any).call(obj, cb, thisArg).join(","));
console.log((Array.prototype.find as any).call(obj, cb, thisArg));
console.log((Array.prototype.findIndex as any).call(obj, cb, thisArg));
console.log((Array.prototype.findLast as any).call(obj, cb, thisArg));
console.log((Array.prototype.findLastIndex as any).call(obj, cb, thisArg));

let seen = 0;
function cb2(this: any) { if (this === thisArg) seen++; }
(Array.prototype.forEach as any).call(obj, cb2, thisArg);
console.log(seen);

// Callback arguments stay `(kValue, k, O)` beside the receiver.
function cb3(this: any, v: any, k: any, o: any) {
  return this === thisArg && v === (k === 0 ? 11 : 22) && o === obj;
}
console.log((Array.prototype.every as any).call(obj, cb3, thisArg));

// No thisArg means `this` is undefined, not the receiver (§10.2.1.2
// on a strict callback).
function cb4(this: any) { return this === undefined; }
console.log((Array.prototype.every as any).call(obj, cb4));

// reduce / reduceRight take an initialValue in that position and call
// with `undefined` (§23.1.3.24 step 6.d.i) — the second argument is
// NOT a receiver there.
function red(this: any, acc: any, v: any) { return acc + (this === undefined ? v : 1000); }
console.log((Array.prototype.reduce as any).call(obj, red, 5));
console.log((Array.prototype.reduceRight as any).call(obj, red, 5));

// A real array receiver already honoured thisArg — it must not move.
console.log([1, 2].map(cb as any, thisArg).join(","));
console.log([1, 2].every(cb4 as any));
