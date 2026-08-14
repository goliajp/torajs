// 404-01 — methods of a GENERIC class dispatch on an any-held
// instance: the specialization row carries the class identity and
// the receiver-polymorphic twin reads fields through the row's own
// per-field tags, so every specialization decodes correctly.
class G<K> {
  k: K;
  constructor(k: K) { this.k = k }
  get(): K { return this.k }
  wrap<T>(t: T): K { return this.k }
  add(n: number, m: number): any { return (this.k as any) + n * m }
}
const g: any = new G<number>(6);
console.log(g.get(), g.wrap(9), g.add(10, 2), g.k);
// two specializations share the method table, each reads its own layout
const s: any = new G<string>("hi");
console.log(s.get(), s.k);
// reflection face rides the same row
console.log(Object.keys(g), JSON.stringify(g));
// typed lane keeps its static retargets, and the boxed value re-answers
const tg = new G<number>(7);
console.log(tg.get());
const h: any = tg;
console.log(h.get());
// an any boundary in between changes nothing
function take(x: any) { return x.get() }
console.log(take(new G<number>(1)), take(new G<string>("s")));
