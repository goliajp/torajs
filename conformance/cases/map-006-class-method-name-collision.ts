// Name-based class-method dispatch must not capture builtin-container
// receivers. desugar rewrites `x.m(a)` → `__cm_<C>__m(x, a)` purely by
// method name (single-owner case); when the receiver's checked type is
// a builtin container (Map / Set / ...) the rewrite is demoted and the
// builtin method runs (speculative_cm_rewrites / demoted_cm_rewrites).

// single-owner collision: Holder.get vs Map.get
class Holder {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
  get(): number {
    return this.v / 2;
  }
}
let h = new Holder(7);
console.log(h.get());

let m = new Map<number, number>();
m.set(1, 2.5);
console.log(m.get(1));

// fract value must survive the width face (Map value slot answers
// width queries even when a user class owns `get` — demoted sites
// carry typed receiver evidence past the any_class_owns_method gate)
let n: number = m.get(1) as number;
console.log(n + 1);

// set/has/delete name faces on the same receiver keep working
console.log(m.has(1));
m.delete(1);
console.log(m.size);

// Set.add collision face: user class owning `add` must not capture
// Set receivers
class Acc {
  total: number;
  constructor() {
    this.total = 0;
  }
  add(x: number): number {
    this.total = this.total + x;
    return this.total;
  }
}
let acc = new Acc();
console.log(acc.add(5));
let st = new Set<number>();
st.add(2.5);
console.log(st.has(2.5));
for (const v of st) {
  console.log(v);
}
// (a Member-shape receiver — `w.inner.get(..)` with `inner: Map` —
// is blocked on the pre-existing `Map<K,V>` class-field-annotation
// gap, tracked in plan-state L3b; the demotion probe accepts Member
// receivers already)
