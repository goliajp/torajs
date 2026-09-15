// §23.1.2.5 / §23.2.2.4 — a subclass answers itself to the species read
// only because it INHERITS the constructor's `get [@@species]`; once
// that accessor is deleted, ArraySpeciesCreate / TypedArraySpeciesCreate
// read undefined and derive the default product.
class MyA extends Array {}
const a: any = MyA.from([1, 2, 3]);
console.log(a.map((x: number) => x * 2) instanceof MyA, a.slice(1) instanceof MyA, a.filter(() => true) instanceof MyA);
class MyI extends Int8Array {}
const t: any = new MyI([1, 2, 3]);
console.log(t.slice(1) instanceof MyI, t.map((x: number) => x) instanceof MyI, t.subarray(1) instanceof MyI);
const A: any = Array;
delete A[Symbol.species];
console.log((MyA as any)[Symbol.species], a.map((x: number) => x) instanceof MyA, Array.isArray(a.slice(1)));
const TA: any = Object.getPrototypeOf(Int8Array);
delete TA[Symbol.species];
const s: any = t.slice(1);
console.log((MyI as any)[Symbol.species], s instanceof MyI, s instanceof Int8Array, s.length);
