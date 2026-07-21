// RFC 20260721-array-proto-cluster 刀 8-B — inline
// `Array.prototype.<m>.call(x)` compile-lane routing (the desugar
// rewrite to `x.m()` retired for the Array namespace) + the concat
// generic arm for non-array object receivers.

// pop on a non-array receiver: length 2 → index 1 absent → answers
// undefined, higher sparse element survives, length decrements.
const p: any = { length: 2, 3: 42 };
console.log("pop:", Array.prototype.pop.call(p), p[3], p.length, 3 in p);

// splice with no args on a non-array receiver: fresh empty Array.
const s: any = { length: 0 };
const sr = Array.prototype.splice.call(s);
console.log("splice:", Array.isArray(sr), sr.length);

// concat on a non-array receiver: receiver seeds as the single
// non-spreadable element, array args spread, others append.
const c: any = { length: 0 };
const cr = Array.prototype.concat.call(c);
console.log("concat:", Array.isArray(cr), cr.length, cr[0] === c);
console.log("concat-proto:", Object.getPrototypeOf(cr) === Array.prototype);
const cr2 = Array.prototype.concat.call(c, [1, 2], "x");
console.log("concat-args:", cr2.length, cr2[0] === c, cr2[1], cr2[2], cr2[3]);

// mutator writeback through the direct form: push growth relocates
// and the caller's binding reads the grown result back.
const arr = [1];
Array.prototype.push.call(arr, 2, 3, 4, 5, 6, 7, 8, 9);
console.log("push-grow:", arr.length, arr[0], arr[8]);

// splice growth through the direct form on a real array.
const a2 = [1, 2];
const rem = Array.prototype.splice.call(a2, 1, 0, 10, 11, 12, 13, 14, 15, 16, 17);
console.log("splice-grow:", a2.length, a2[0], a2[1], a2[9], rem.length);

// shift on a non-array receiver.
const sh: any = { length: 1, 0: "z" };
console.log("shift:", Array.prototype.shift.call(sh), sh.length);

// direct triple-member read still answers a callable value.
console.log("typeof:", typeof Array.prototype.pop);
