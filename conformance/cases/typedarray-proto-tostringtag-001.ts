// §23.2.3.38 — %TypedArray.prototype%[@@toStringTag] is an accessor.
const TA: any = Object.getPrototypeOf(Int8Array);
const P: any = TA.prototype;
const d: any = Object.getOwnPropertyDescriptor(P, Symbol.toStringTag);
console.log(typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length);
const kinds: any[] = [Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array, Int32Array, Uint32Array, Float32Array, Float64Array, BigInt64Array, BigUint64Array];
for (const K of kinds) {
  const t = new K(1);
  console.log(t[Symbol.toStringTag], Object.prototype.toString.call(t), d.get.call(t));
}
console.log(d.get.call(undefined), d.get.call(3), d.get.call({}), d.get.call(new DataView(new ArrayBuffer(2))), P[Symbol.toStringTag], (Int8Array.prototype as any)[Symbol.toStringTag]);
console.log(Object.getOwnPropertySymbols(P).length, Object.getOwnPropertySymbols(Int8Array.prototype).length, Symbol.toStringTag in new Int8Array(1));
console.log(Object.prototype.toString.call(P), Object.prototype.toString.call(Int8Array.prototype));
delete P[Symbol.toStringTag];
console.log(Object.prototype.toString.call(new Int8Array(1)), (new Int8Array(1) as any)[Symbol.toStringTag]);
