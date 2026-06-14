// W-O-3-arr — Object.entries(arr) returns [[idx_str, value], ...]
// Spec ES §20.1.2.5 + ToObject on an Array exotic: returns Arr<Arr<2>>
// where each entry is [string-index, value]. tora's helper builds the
// outer Arr<Arr<Any>> via arr_alloc + per-entry arr_alloc_any(2);
// the SSA arm picks the per-element NaN-box tag (Bool=1 / I64=2 /
// F64=3 / refcounted=4) from the typed Arr's element type.
//
// Note: console.log on the full Arr<Arr<Any>> shape currently prints
// as a raw pointer (pre-existing on Object.entries(struct) too — L3b
// W-O-3-nested-print). Fixture probes via typed-var indexed access
// so the value extraction round-trips cleanly.

const a: number[] = [10, 20];
const ea = Object.entries(a);
console.log(ea.length);
const ea0 = ea[0];
const ea1 = ea[1];
console.log(ea0[0]);
console.log(ea0[1]);
console.log(ea1[0]);
console.log(ea1[1]);

const b: number[] = [];
const eb = Object.entries(b);
console.log(eb.length);

const d: string[] = ["x", "y"];
const ed = Object.entries(d);
console.log(ed.length);
const ed0 = ed[0];
const ed1 = ed[1];
console.log(ed0[0]);
console.log(ed0[1]);
console.log(ed1[0]);
console.log(ed1[1]);
