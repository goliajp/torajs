// RFC 20260916-typedarray-intrinsic 刀 2 — %TypedArray.prototype%'s four
// accessors: descriptor shape, brand checks, values on a real view, and
// HasProperty reaching them through a per-kind prototype.
const TA: any = Object.getPrototypeOf(Int8Array);
const P: any = TA.prototype;
for (const k of ["buffer", "byteLength", "byteOffset", "length"]) {
  const d: any = Object.getOwnPropertyDescriptor(P, k);
  console.log(k, typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length);
  try { P[k]; console.log("no throw"); } catch (e) { console.log("proto read", (e as any).constructor.name); }
  try { d.get.call(new DataView(new ArrayBuffer(8))); } catch (e) { console.log("dv", (e as any).constructor.name); }
  try { d.get.call({}); } catch (e) { console.log("obj", (e as any).constructor.name); }
  try { d.get.call(undefined); } catch (e) { console.log("undef", (e as any).constructor.name); }
}
const buf = new ArrayBuffer(16);
const ta = new Int32Array(buf, 4, 2);
const g = (k: string) => (Object.getOwnPropertyDescriptor(P, k) as any).get.call(ta);
console.log(g("length"), g("byteLength"), g("byteOffset"), g("buffer") === buf);
console.log(P.hasOwnProperty("length"), (Int32Array.prototype as any).hasOwnProperty("length"));
for (const k of ["length", "buffer", "byteOffset", "at", "constructor", "nope"]) {
  console.log(k, k in Int32Array.prototype, k in P, Reflect.has(Uint8Array.prototype, k));
}
