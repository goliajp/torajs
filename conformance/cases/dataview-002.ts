// RFC 20260823-typedarray-substrate 刀 7 second half — the §25.3.4
// get*/set* accessor methods and per-call endianness.
const b = new ArrayBuffer(16);
const dv = new DataView(b);

// Round trips, little-endian.
dv.setInt8(0, -2);
console.log(dv.getInt8(0), dv.getUint8(0));
dv.setInt16(0, -2, true);
console.log(dv.getInt16(0, true), dv.getUint16(0, true));
// The same bytes read with the other endianness.
console.log(dv.getInt16(0, false), dv.getUint16(0, false));
// Byte-level view of what setInt16 LE wrote.
console.log(dv.getUint8(0), dv.getUint8(1));

// Endianness defaults to BIG when omitted.
dv.setUint16(2, 0x1234);
console.log(dv.getUint8(2).toString(16), dv.getUint8(3).toString(16));
console.log(dv.getUint16(2, true).toString(16));

// 32-bit and floats.
dv.setInt32(4, -123456789, true);
console.log(dv.getInt32(4, true), dv.getUint32(4, true));
dv.setFloat32(8, 1.5, true);
console.log(dv.getFloat32(8, true));
dv.setFloat64(8, 1 / 3, true);
console.log(dv.getFloat64(8, true));
dv.setFloat16(0, 1.5);
console.log(dv.getFloat16(0));

// BigInt kinds.
dv.setBigInt64(8, -2n, true);
console.log(dv.getBigInt64(8, true), dv.getBigUint64(8, true));
dv.setBigUint64(8, 18446744073709551615n, true);
console.log(dv.getBigUint64(8, true), dv.getBigInt64(8, true));

// ToInt wrapping on stores.
dv.setInt8(0, 300);
console.log(dv.getInt8(0), dv.getUint8(0));
dv.setUint8(0, -1);
console.log(dv.getUint8(0));

// Range and coercion rejections.
try {
  dv.getInt32(13);
} catch (e) {
  console.log("range", (e as Error).constructor.name);
}
try {
  dv.getInt8(-1);
} catch (e) {
  console.log("neg", (e as Error).constructor.name);
}
try {
  dv.setInt8(16, 1);
} catch (e) {
  console.log("setrange", (e as Error).constructor.name);
}
try {
  dv.setBigInt64(0, 1 as any);
} catch (e) {
  console.log("notbigint", (e as Error).constructor.name);
}
try {
  dv.setInt8(0, 1n as any);
} catch (e) {
  console.log("notnumber", (e as Error).constructor.name);
}

// Out-of-bounds / detached views throw TypeError from every accessor.
const rb = new ArrayBuffer(4, { maxByteLength: 8 });
const fdv = new DataView(rb, 2, 2);
rb.resize(1);
try {
  fdv.getUint8(0);
} catch (e) {
  console.log("oob", (e as Error).constructor.name);
}
const b2 = new ArrayBuffer(4);
const ddv = new DataView(b2);
b2.transfer();
try {
  ddv.setInt8(0, 1);
} catch (e) {
  console.log("det", (e as Error).constructor.name);
}

// The methods read as values and on the has face.
console.log(typeof dv.getInt8, typeof dv.setFloat64);
console.log("getInt8" in dv, "setBigUint64" in dv, "getNope" in dv);

// A length-tracking view grows with its buffer, methods included.
const rb2 = new ArrayBuffer(2, { maxByteLength: 8 });
const tdv = new DataView(rb2);
rb2.resize(8);
tdv.setFloat64(0, 2.5, true);
console.log(tdv.getFloat64(0, true));
