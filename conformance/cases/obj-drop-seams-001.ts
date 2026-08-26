// r502 — a struct instance's typed drop keeps three speculative legs
// behind link seams: the expando-bag release, the cycle-root
// buffering of a fields-all-scalar instance, and the FLAG_BUFFERED
// scrub. Every leg below must run for real — an instance grows a bag
// through the any-world member set, a symbol-keyed set,
// Object.defineProperty and an Error's cause, and a bag-carried
// cycle must still be collected. A stripped seam answers a named
// TypeError on the drop, never a silent leak.
class P {
  x = 1;
  y = 2;
  sum() {
    return this.x + this.y;
  }
}
let plain = 0;
for (let i = 0; i < 300; i++) {
  const p = new P();
  p.x = i;
  plain += p.sum();
}
console.log(plain);
let tagged = 0;
for (let i = 0; i < 300; i++) {
  const p = new P();
  (p as any).extra = i;
  tagged += (p as any).extra + p.sum();
}
console.log(tagged);
const sym = Symbol("s");
const q = new P();
(q as any)[sym] = 41;
console.log((q as any)[sym] + 1);
const d = new P();
Object.defineProperty(d, "k", { value: 7, enumerable: true });
console.log((d as any).k, Object.keys(d).length);
for (let i = 0; i < 100; i++) {
  const a = new P();
  (a as any).self = a;
}
console.log("cycles dropped");
const e = new Error("boom", { cause: new P() });
console.log((e.cause as P).sum());
