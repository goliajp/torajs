// §23.1.3.11 — flatMap threads its thisArg (T) into a promoted
// fn-expr callback; an Any-returning callback runs the step-8 IsArray
// test per element at runtime (array answers spread, anything else
// pushes as the element itself).
const xs = [1, 2];
const ctx = { k: 10 };
const r = xs.flatMap(function (v: number) {
  return [v + this.k];
}, ctx);
console.log(r.length, r[0], r[1]);
const ctx2 = { k: 7 };
const mixed = xs.flatMap(function (v: number) {
  return v === 2 ? [v, this.k] : this.k;
}, ctx2);
console.log(mixed.length, mixed[0], mixed[1], mixed[2]);
