// any-method-call RFC C4+ chunk 523 — getter-as-callee: an accessor
// property answering a closure invokes as a method; non-fn getter
// answers raise the catchable TypeError.
const p: any = {};
const g = (x: number) => x * 2;
Object.defineProperty(p, "m", { get: () => g });
console.log(p.m(21));
// member read then bare call rides the read fallback's accessor path
const h: any = p.m;
console.log(h(3));
// getter runs per call
let hits = 0;
const q: any = {};
Object.defineProperty(q, "f", {
  get: () => {
    hits = hits + 1;
    return g;
  },
});
console.log(q.f(1));
console.log(q.f(2));
console.log(hits);
// a getter answering a non-function raises the TypeError
const r: any = {};
Object.defineProperty(r, "n", { get: () => 5 });
try {
  r.n(1);
} catch (e) {
  console.log("caught");
}
console.log("done");
