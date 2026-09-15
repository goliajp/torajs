// §23.2.2 / §23.2.6 — `from` / `of` are own statics of the abstract
// %TypedArray% only; the twelve concrete constructors inherit them.
const TA: any = Object.getPrototypeOf(Int8Array);
const I8: any = Int8Array;
const d: any = Object.getOwnPropertyDescriptor(TA, "from");
console.log(typeof d.value, d.writable, d.enumerable, d.configurable, TA.from.name, TA.from.length, TA.of.length);
console.log(I8.hasOwnProperty("from"), I8.hasOwnProperty("of"), Object.getOwnPropertyDescriptor(I8, "of"), "from" in I8, Reflect.has(Float64Array, "of"));
console.log(I8.from === TA.from, Uint16Array.of === TA.of, (Int8Array as any)["of"] === TA.of);
console.log(Int8Array.from([1, 2, 3]).join(","), Float32Array.of(1.5, 2).join(","), I8["of"](7, 8).join(","), I8.from.call(Uint8Array, [9]).constructor === Uint8Array);
console.log(delete I8.from, typeof I8.from, TA.hasOwnProperty("from"));
console.log(delete TA.of, TA.hasOwnProperty("of"), typeof I8.of, "of" in Uint8Array);
