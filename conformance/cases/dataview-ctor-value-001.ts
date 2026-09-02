// 565-01 — DataView was the one bound builtin constructor with no row
// in the name / ctor-clause-length table: `console.log(DataView)`
// printed the bare `[Function]`, `.name` answered `""` and `.length`
// `undefined`. Its prototype slot sits after the eleven per-kind
// typed-array slots, and the table simply stopped before it.
//
// Two things were reading past the same end. `new dv(buf)` through a
// value binding computed a typed-array element kind as `tag - 20`,
// which for DataView's slot is 12 — one past the last kind — and
// minted a Float16Array. And an instance's family tag had no arm at
// all, so the any-lane `v.constructor` walked off the end and threw
// on a null receiver.
const b = new ArrayBuffer(8);
const v: any = new DataView(b);
v.setInt32(0, 7);
v.setUint8(4, 255);

const dv: any = DataView;
console.log(dv, JSON.stringify(dv.name), JSON.stringify(dv.length));
console.log(DataView.name, DataView.length);

console.log(v.getInt32(0), v.getUint8(4), v instanceof DataView);
console.log(v.constructor === DataView, JSON.stringify(v.constructor.name));
console.log(Object.getPrototypeOf(v) === DataView.prototype);

// [[Construct]] through the value binding is the same constructor
const w: any = new dv(b, 4, 4);
console.log(w.getUint8(0), w.byteLength, w.byteOffset, w.constructor === DataView);

// the siblings the slot sits between are unchanged
console.log(ArrayBuffer, Uint8Array, Float16Array);
console.log(JSON.stringify(ArrayBuffer.name), ArrayBuffer.length);
const u: any = new Uint8Array(2);
console.log(u.constructor === Uint8Array, (new ArrayBuffer(2) as any).constructor === ArrayBuffer);
