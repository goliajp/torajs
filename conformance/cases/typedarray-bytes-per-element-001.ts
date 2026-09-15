// §23.2.6.1 / §23.2.7.1 — BYTES_PER_ELEMENT is an own, non-writable,
// non-enumerable, non-configurable data property of each typed-array
// constructor and of each per-kind prototype, and of neither
// %TypedArray% nor %TypedArray.prototype%. Number's §21.1.2 constants
// share the same refusal faces.
const kinds: any[] = [Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array, Int32Array, Uint32Array, Float32Array, Float64Array, BigInt64Array, BigUint64Array];
for (const K of kinds) {
  const c: any = Object.getOwnPropertyDescriptor(K, "BYTES_PER_ELEMENT");
  const p: any = Object.getOwnPropertyDescriptor(K.prototype, "BYTES_PER_ELEMENT");
  console.log(K.name, K.BYTES_PER_ELEMENT, K.prototype.BYTES_PER_ELEMENT, JSON.stringify(c), JSON.stringify(p));
}
const TA: any = Object.getPrototypeOf(Int8Array);
console.log(TA.hasOwnProperty("BYTES_PER_ELEMENT"), TA.prototype.hasOwnProperty("BYTES_PER_ELEMENT"), TA.prototype.BYTES_PER_ELEMENT);
const t: any = new Int16Array(2);
console.log(t.BYTES_PER_ELEMENT, t.hasOwnProperty("BYTES_PER_ELEMENT"), "BYTES_PER_ELEMENT" in t);
const U: any = Uint8Array;
try { U.BYTES_PER_ELEMENT = 9; console.log("write ok"); } catch (e) { console.log("ctor write", (e as any).constructor.name); }
try { U.prototype.BYTES_PER_ELEMENT = 9; console.log("write ok"); } catch (e) { console.log("proto write", (e as any).constructor.name); }
try { delete U.BYTES_PER_ELEMENT; console.log("delete ok"); } catch (e) { console.log("ctor delete", (e as any).constructor.name); }
try { delete U.prototype.BYTES_PER_ELEMENT; console.log("delete ok"); } catch (e) { console.log("proto delete", (e as any).constructor.name); }
console.log(U.BYTES_PER_ELEMENT, U.prototype.BYTES_PER_ELEMENT, Object.getOwnPropertyNames(U.prototype).sort().join(","));
const N: any = Number;
try { N.MAX_SAFE_INTEGER = 1; console.log("write ok"); } catch (e) { console.log("number write", (e as any).constructor.name); }
try { delete N.EPSILON; console.log("delete ok"); } catch (e) { console.log("number delete", (e as any).constructor.name); }
console.log(N.MAX_SAFE_INTEGER, N.EPSILON > 0);
