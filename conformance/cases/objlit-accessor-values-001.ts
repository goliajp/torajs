// RFC 20260714-objlit-accessor blade 6 — Object.values / Object.entries
// reach an accessor through [[Get]]. A layout slot is not a property:
// `__getter_v` is one half of the property `v`, and its value is what
// the getter answers, not the closure sitting in the slot.

const o = {
  a: 1,
  get v(): number {
    return 2;
  },
};
// typed lane (compile-time unfold) — `values` used to be rejected
// outright ("requires homogeneous struct fields"), `entries` used to
// emit the synthetic slot name as the key.
console.log(Object.values(o));
console.log(Object.entries(o));

// `any` lane (runtime layout walker) — the key was already right, the
// value was the getter closure.
const oa: any = o;
console.log(Object.values(oa));
console.log(Object.entries(oa));

// a get/set pair is ONE own property, in both lanes (the values walk
// had no dedup at all and counted it twice).
const p = {
  a: 1,
  get v(): number {
    return this.a;
  },
  set v(n: number) {
    this.a = n;
  },
};
const pa: any = p;
console.log(Object.keys(p), Object.values(p), Object.entries(p));
console.log(Object.values(pa), Object.entries(pa));

// the getter sees its `this`, and a heap result outlives the struct
// (the array takes the getter's own reference — no double-free, no
// borrow of a slot the struct is about to release).
function mk(): string[] {
  const g = {
    s: "a" + "b",
    get up(): string {
      return this.s + "!";
    },
  };
  return Object.values(g);
}
console.log(mk());

const h = {
  s: "hi",
  get greet(): string {
    return "hello " + this.s;
  },
};
console.log(Object.values(h), Object.entries(h));
