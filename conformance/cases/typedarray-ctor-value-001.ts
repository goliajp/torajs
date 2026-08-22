// §23.2.5 / §25.1.4 — the twelve buffer-family constructors read as
// VALUES. Everything below is what a table of constructors needs to
// work at all, which is the shape test262's own typed-array harness
// is built out of.
const ctors = [Float64Array, Float32Array, Int32Array, Int16Array, Int8Array];
const more = ctors.concat([Uint32Array, Uint16Array, Uint8Array, Uint8ClampedArray] as any);
const all = more.concat([BigInt64Array, BigUint64Array] as any);
console.log(ctors.length, more.length, all.length);
console.log(ctors[0] === Float64Array, ctors[0] === Float32Array);

// name / length / BYTES_PER_ELEMENT off a runtime constructor value.
for (let i = 0; i < all.length; i++) {
  const C: any = all[i];
  console.log(C.name, C.length, C.BYTES_PER_ELEMENT, new C(2).length, new C(2).byteLength);
}
console.log(ArrayBuffer.name, ArrayBuffer.length);
const AB: any = ArrayBuffer;
console.log(new AB(8).byteLength, new AB(8) instanceof ArrayBuffer);

// The static face answers the same numbers as the value face.
console.log(
  Uint8Array.BYTES_PER_ELEMENT,
  Int16Array.BYTES_PER_ELEMENT,
  Float32Array.BYTES_PER_ELEMENT,
  Float64Array.BYTES_PER_ELEMENT,
  BigUint64Array.BYTES_PER_ELEMENT,
);
console.log(new Uint8Array(1).BYTES_PER_ELEMENT, new Float64Array(1).BYTES_PER_ELEMENT);

// `typeof` and the instance's `.constructor` back-reference.
console.log(typeof Uint8Array, typeof ArrayBuffer, typeof BigInt64Array);
console.log(new Uint8Array(1).constructor === Uint8Array);
console.log(new Uint8Array(1).constructor === Int8Array);
console.log(new ArrayBuffer(1).constructor === ArrayBuffer);

// Each constructor has its own prototype object, and
// `ArrayBuffer.prototype` carries the two methods §25.1.6 gives it.
console.log(typeof ArrayBuffer.prototype, typeof Uint8Array.prototype);
console.log(Uint8Array.prototype === Int8Array.prototype);
console.log(typeof ArrayBuffer.prototype.resize, typeof ArrayBuffer.prototype.slice);
console.log(ArrayBuffer.prototype.hasOwnProperty("slice"), ArrayBuffer.prototype.hasOwnProperty("nope"));

// §25.1.4.1 / §23.2.5.1 step 1 — all twelve require `new`.
try { (Uint8Array as any)(1); } catch (e) { console.log((e as Error).constructor.name); }
try { (ArrayBuffer as any)(1); } catch (e) { console.log((e as Error).constructor.name); }
try { (BigInt64Array as any)(1); } catch (e) { console.log((e as Error).constructor.name); }
