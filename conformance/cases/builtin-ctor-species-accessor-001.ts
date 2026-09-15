// §23.1.2.5 / §22.2.5.2 / §27.2.4.8 / §24.1.2.3 / §24.2.2.3 / §25.1.5.3 /
// §23.2.2.4 — each species-aware builtin constructor owns a get-only
// `@@species` accessor that returns `this`; a subclass inherits it and
// answers itself, a concrete typed-array constructor inherits
// %TypedArray%'s.
const ctors: any[] = [Array, RegExp, Promise, Map, Set, ArrayBuffer, Object.getPrototypeOf(Int8Array)];
for (const c of ctors) {
  const d: any = Object.getOwnPropertyDescriptor(c, Symbol.species);
  console.log(c.name, typeof d.get, d.set, d.enumerable, d.configurable, d.get.name, d.get.length);
  const o = {};
  console.log(" this", d.get.call(o) === o, c[Symbol.species] === c, Symbol.species in c);
}
const I8: any = Int8Array;
console.log(I8[Symbol.species] === I8, Object.prototype.hasOwnProperty.call(I8, Symbol.species));
class MyA extends Array {}
console.log((MyA as any)[Symbol.species] === MyA);
const a: any = [1, 2, 3];
a.constructor = Array;
console.log(a.map((x: number) => x * 2).join(), a.slice(1).join(), a.concat([4]).length);
const t: any = new Int8Array([1, 2, 3]);
t.constructor = Int8Array;
console.log(t.slice(1).join(), t.map((x: number) => x + 1) instanceof Int8Array);
Promise.resolve(5)
  .finally(() => console.log("finally"))
  .then((v: any) => console.log("then", v));
console.log(Object.getOwnPropertySymbols(Map).length, Object.keys(Set).length);
const A2: any = Array;
console.log(delete A2[Symbol.species], Symbol.species in A2, A2[Symbol.species]);
