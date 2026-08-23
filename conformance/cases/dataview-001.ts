// RFC 20260823-typedarray-substrate 刀 7 — DataView cell, ctor,
// accessors, ordinary-object face. Methods (get*/set*) are the 刀 7
// second half.
const b = new ArrayBuffer(8);
const dv = new DataView(b);
console.log(typeof dv);
console.log(dv instanceof DataView);
console.log(Object.prototype.toString.call(dv));
console.log(dv.byteLength, dv.byteOffset);
console.log(dv.buffer === b);
console.log(dv);
console.log([dv]);

// Sub-range views.
const dv2 = new DataView(b, 3);
console.log(dv2.byteLength, dv2.byteOffset);
const dv3 = new DataView(b, 2, 4);
console.log(dv3.byteLength, dv3.byteOffset);

// Constructor rejections.
try {
  new DataView({} as any);
} catch (e) {
  console.log("nonbuf", (e as Error).constructor.name);
}
try {
  new DataView(b, 9);
} catch (e) {
  console.log("offrange", (e as Error).constructor.name);
}
try {
  new DataView(b, 2, 7);
} catch (e) {
  console.log("lenrange", (e as Error).constructor.name);
}

// Length-tracking over a resizable buffer.
const rb = new ArrayBuffer(4, { maxByteLength: 8 });
const tdv = new DataView(rb);
console.log(tdv.byteLength);
rb.resize(6);
console.log(tdv.byteLength);
// A fixed-length view that falls out of bounds answers TypeError.
const fdv = new DataView(rb, 2, 4);
rb.resize(3);
try {
  console.log(fdv.byteLength);
} catch (e) {
  console.log("oob", (e as Error).constructor.name);
}

// Detach via transfer: accessors throw, buffer identity survives,
// print shows length zero.
const b2 = new ArrayBuffer(4);
const ddv = new DataView(b2);
b2.transfer();
try {
  console.log(ddv.byteLength);
} catch (e) {
  console.log("detbl", (e as Error).constructor.name);
}
try {
  console.log(ddv.byteOffset);
} catch (e) {
  console.log("detbo", (e as Error).constructor.name);
}
console.log(ddv.buffer === b2);
console.log(ddv);

// Ordinary-object face — expandos land in the bag, numeric keys
// included (a DataView is NOT integer-indexed exotic).
const xdv = new DataView(b) as any;
xdv.x = 1;
xdv[0] = 5;
console.log(xdv.x, xdv[0]);
console.log(Object.keys(xdv));
console.log("x" in xdv, 0 in xdv, "byteLength" in xdv, "nope" in xdv);
Object.defineProperty(xdv, "y", { value: 7, enumerable: false });
console.log(xdv.y, Object.keys(xdv));
const d = Object.getOwnPropertyDescriptor(xdv, "y");
console.log(d && d.value, d && d.enumerable);
console.log(delete xdv.x, xdv.x);
// The numeric expando never touched the buffer's bytes.
console.log(new DataView(b).byteLength, b.byteLength);
console.log(Object.getOwnPropertyDescriptor(xdv, "byteLength"));
