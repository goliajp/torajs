// TypedArray exotic subclass — basic mint / element face / identity
// (RFC 20260730-exotic-backed-class-instance, buffer-family blade).
class MyUint8Array extends Uint8Array {}
const a = new MyUint8Array(4);
a[0] = 7; a[1] = 9;
console.log(a[0], a[1], a.length);
console.log(a instanceof MyUint8Array, a instanceof Uint8Array);
console.log(Object.getPrototypeOf(a) === MyUint8Array.prototype);
console.log(a.byteLength, a.byteOffset);
const s = a.subarray(1);
console.log(s[0], s.constructor === MyUint8Array);
console.log(a.constructor === MyUint8Array);
