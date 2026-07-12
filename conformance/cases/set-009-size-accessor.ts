// get Map.prototype.size / get Set.prototype.size (§24.1.3.10 /
// §24.2.3.9) — gOPD synthesizes the spec accessor descriptor, the
// getter cells are distinct per prototype, callable via .call, and
// the own-property probes and delete tombstone agree.

const d = Object.getOwnPropertyDescriptor(Set.prototype, "size") as any;
console.log(typeof d.get, typeof d.set, d.enumerable, d.configurable);
console.log(d.get.name, d.get.length);

const dm = Object.getOwnPropertyDescriptor(Map.prototype, "size") as any;
console.log(typeof dm.get, dm.get.name, d.get === dm.get);

// getter invocation through .call
const s = new Set([1, 2, 3]);
console.log(d.get.call(s));
const m = new Map<any, any>([["a", 1]]);
console.log(dm.get.call(m));

// own-property probes
console.log(Object.prototype.hasOwnProperty.call(Set.prototype, "size"));
console.log((Set.prototype as any).propertyIsEnumerable("size"));

// delete tombstones the accessor; defineProperty restores it
delete (Set.prototype as any).size;
console.log(
  Object.getOwnPropertyDescriptor(Set.prototype, "size") === undefined,
  Object.prototype.hasOwnProperty.call(Set.prototype, "size"),
);
Object.defineProperty(Set.prototype, "size", {
  get: d.get,
  enumerable: false,
  configurable: true,
});
const d2 = Object.getOwnPropertyDescriptor(Set.prototype, "size") as any;
console.log(typeof d2.get, d2.get === d.get);
