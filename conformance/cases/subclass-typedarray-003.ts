// TypedArray subclass over resizable buffers, BigInt kinds, an
// explicit ctor, a user method, and the from-array form — the
// resizableArrayBufferUtils harness shapes.
class MyFloat32Array extends Float32Array {}
class MyBigInt64Array extends BigInt64Array {}
const rab = new ArrayBuffer(16, { maxByteLength: 40 });
const f = new MyFloat32Array(rab, 0, 2);
f[0] = 1.5;
console.log(f[0], f.length, f instanceof MyFloat32Array);
const t = new MyFloat32Array(rab);
console.log(t.length);
rab.resize(24);
console.log(t.length);
const b = new MyBigInt64Array(2);
b[0] = 7n;
console.log(b[0], b instanceof BigInt64Array);
class WithCtor extends Uint8Array {
  constructor(n: any) { super(n); }
}
const w = new WithCtor(3);
console.log(w.length);
class WithMethod extends Uint8Array {
  sum() { let s = 0; for (let i = 0; i < this.length; i++) s += this[i]; return s; }
}
const m = new WithMethod(3);
m[0] = 4; m[2] = 8;
console.log(m.sum());
const src = [1, 2, 3];
class MyU8 extends Uint8Array {}
const fromArr = new MyU8(src);
console.log(fromArr.length, fromArr[1]);
