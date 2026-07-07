// chunk 634 — owned-temp keys settle AFTER the weak-collection
// kernel call: a setter's fresh key (`new K()`, `Symbol()`) held no
// outside reference, so the entry evicts the moment the temp dies
// (was: object-like fast path never settled = leak; Symbol lane
// settled BEFORE the kernel = dangling entry). Behavior with kept
// keys must be untouched.
class K {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}
const wm = new WeakMap<K, number>();
const keep = new K(7);
wm.set(keep, 70);
wm.set(new K(1), 10);
console.log(wm.get(keep));
console.log(wm.has(keep));
wm.has(new K(2));
console.log(wm.has(keep));
const ws = new WeakSet<K>();
const kept2 = new K(8);
ws.add(kept2);
ws.add(new K(3));
console.log(ws.has(kept2));
const wms = new WeakMap<symbol, string>();
const skeep = Symbol("keep");
wms.set(skeep, "v");
wms.set(Symbol("temp"), "gone");
console.log(wms.get(skeep));
console.log(wms.has(skeep));
console.log("end");
