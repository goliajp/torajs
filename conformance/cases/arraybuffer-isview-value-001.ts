// §25.1.5.1 — `ArrayBuffer.isView` read as a VALUE. The direct call
// lowers per-shape, but the constructor reached through an `any` went
// to the runtime ctor-static lookup, which had no "ArrayBuffer"
// namespace at all: the member read answered `undefined` and calling
// it was a loud TypeError. DataView stays out of that map on purpose
// — the spec gives it no statics.
const AB: any = ArrayBuffer;
const v: any = AB.isView;

console.log("typeof", typeof v);
console.log("name", v.name, "length", v.length);
// Receiver-less per §25.1.5.1 — a detached call is the real kernel.
console.log("detached-ta", v(new Uint8Array(4)));
console.log("detached-dv", v(new DataView(new ArrayBuffer(8))));
console.log("detached-ab", v(new ArrayBuffer(8)));
console.log("detached-num", v(1), "detached-none", v());
// Same interned identity the direct read answers.
console.log("identity", v === (ArrayBuffer as any).isView);
console.log("gopd", (Object.getOwnPropertyDescriptor(AB, "isView") as any).value === v);
console.log("ctor?", (v as any).prototype === undefined);
console.log("direct", ArrayBuffer.isView(new Int16Array(2)));
