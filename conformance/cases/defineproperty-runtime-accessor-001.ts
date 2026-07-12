// runtime (non-literal) accessor descriptors — RFC
// 20260712-object-create-define-props chunk 2: get/set read off the
// desc dynobj at runtime, AccessorPair built and stored
const o: any = {};
// getter
const dget: any = { get: () => 42, enumerable: true, configurable: true };
Object.defineProperty(o, "g", dget);
console.log(o.g);
// setter
let captured: any = 0;
const dset: any = { set: (v: any) => { captured = v; } };
Object.defineProperty(o, "s", dset);
o.s = 7;
console.log(captured);
// both, closure-captured state
let cell: any = 1;
const dboth: any = { get: () => cell, set: (v: any) => { cell = v * 2; } };
Object.defineProperty(o, "b", dboth);
o.b = 5;
console.log(o.b);
// gOPD readback of a runtime-defined accessor
const d: any = Object.getOwnPropertyDescriptor(o, "g");
console.log(typeof d.get, d.enumerable, d.configurable);
// explicit undefined getter with setter present
const dsu: any = { get: undefined, set: (v: any) => { captured = v; } };
Object.defineProperty(o, "u", dsu);
o.u = 11;
console.log(captured);
// mixed accessor + data → TypeError
const dmix: any = { get: () => 1, value: 2 };
try {
  Object.defineProperty(o, "bad", dmix);
  console.log("no-throw");
} catch (e) {
  console.log("mixed", e instanceof TypeError);
}
// mixed accessor + writable → TypeError
const dmw: any = { set: (v: any) => {}, writable: true };
try {
  Object.defineProperty(o, "bad2", dmw);
  console.log("no-throw");
} catch (e) {
  console.log("mixed-w", e instanceof TypeError);
}
// non-callable getter → TypeError
const dnc: any = { get: 5 };
try {
  Object.defineProperty(o, "bad3", dnc);
  console.log("no-throw");
} catch (e) {
  console.log("noncallable", e instanceof TypeError);
}
// data-descriptor regression through the same runtime lane
const ddata: any = { value: 9, writable: true, enumerable: true, configurable: true };
Object.defineProperty(o, "v", ddata);
console.log(o.v);
