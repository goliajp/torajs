// §7.3.11 HasProperty does not stop at own. A hole is absent from the
// receiver, and the question then goes to the prototype — and
// `Array.prototype` is an Arr cell of its own, so a digit key installed
// on it makes the receiver's hole a property after all.
//
// And §10.4.2.1 raises `length` to cover a defined index while creating
// nothing in between, so the gap a define walks past is holes. Both
// gap-fill paths used to leave own `undefined`s there; on the shared
// `Array.prototype` that made index 0 own for every array in the
// program.
//
// The accumulators are strings, not arrays: a digit key on
// `Array.prototype` is inherited by every array, so pushing into one
// would be answering a different question.
console.log("proto len :", Array.prototype.length);
Object.defineProperty(Array.prototype, "1", { set: function () {}, configurable: true });
console.log("grown len :", Array.prototype.length);
console.log("gap own   :", Object.prototype.hasOwnProperty.call(Array.prototype, "0"));
console.log("defined   :", Object.prototype.hasOwnProperty.call(Array.prototype, "1"));

// Index 1 of the receiver is a hole, but the prototype supplies it, so
// the callback runs there and reads `undefined` through the accessor.
let a: any[] = [1, 2, 3];
delete a[1];
let s: string = "";
a.forEach(function (v: any, i: number) { s = s + String(i) + ":" + String(typeof v) + " "; });
console.log("inherited :", s);

// Index 0 is a hole nothing supplies — the prototype's own index 0 is
// part of the gap, not a property.
let b: any[] = [, 9];
let t: string = "";
b.forEach(function (v: any, i: number) { t = t + String(i) + " "; });
console.log("gap absent:", t);
