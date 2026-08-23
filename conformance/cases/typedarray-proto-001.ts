// RFC 20260823-typedarray-substrate — the buffer family answers its
// [[Prototype]]: per-kind slots for the views, ArrayBuffer at 19,
// DataView after the per-kind block. The chain above a per-kind
// prototype is %Object.prototype% (recorded: no %TypedArray%
// intermediate yet, the iterator-proto shape).
const ta = new Int8Array(2);
console.log(Object.getPrototypeOf(ta) === Int8Array.prototype);
const u8 = new Uint8Array(2);
console.log(Object.getPrototypeOf(u8) === Uint8Array.prototype, Object.getPrototypeOf(u8) === Int8Array.prototype);
const ab = new ArrayBuffer(4);
console.log(Object.getPrototypeOf(ab) === ArrayBuffer.prototype);
const dv = new DataView(ab);
console.log(Object.getPrototypeOf(dv) === DataView.prototype);
console.log(ta instanceof Int8Array, ta instanceof Object);
console.log(Object.getPrototypeOf(Object.getPrototypeOf(ab)) === Object.prototype);
console.log(Object.prototype.toString.call(Int8Array.prototype));
console.log(Object.prototype.toString.call(DataView.prototype));
console.log("end");
