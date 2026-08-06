// §10.1.6.3 over a declared ACCESSOR member of a struct cell. Its
// getter and setter stay in their layout slots; only the attributes
// move, and they move into the same sidecar a declared data field
// uses — `enumerable` and `configurable` are real questions about an
// accessor and a layout row can express neither.

let seen = 0;
const o = {
  get p() {
    return 1;
  },
  set p(v: number) {
    seen = v;
  },
  q: 2,
};
const oa = o as any;

const d0 = Object.getOwnPropertyDescriptor(oa, "p");
console.log(typeof d0.get, typeof d0.set, d0.enumerable, d0.configurable);

// Attributes only — the faces are untouched, so both still work.
Object.defineProperty(oa, "p", { enumerable: false });
console.log(oa.p, Object.keys(oa).join(","));
oa.p = 42;
console.log(seen, oa.p);

const d1 = Object.getOwnPropertyDescriptor(oa, "p");
console.log(typeof d1.get, typeof d1.set, d1.enumerable, d1.configurable);

// Every enumerable-only surface agrees with the descriptor.
console.log(JSON.stringify(oa));
console.log(oa.propertyIsEnumerable("p"), oa.propertyIsEnumerable("q"));
const names: string[] = [];
for (const k in oa) names.push(k);
console.log(names.join(","));
// (Object.entries / Object.values over a STATICALLY typed receiver
// unfold the layout at compile time and do not yet consult the
// sidecar — the same gap for a redefined data field, covered by the
// next fixture.)
// gOPN keeps the non-enumerable one; "p" is still an own property.
console.log(Object.getOwnPropertyNames(oa).join(","), "p" in oa);

// A non-configurable accessor refuses the next enumerable change
// (§10.1.6.3 step 4) and refuses to be deleted.
Object.defineProperty(oa, "p", { configurable: false });
console.log(Object.getOwnPropertyDescriptor(oa, "p").configurable);
try {
  Object.defineProperty(oa, "p", { enumerable: true });
  console.log("redefine: no throw");
} catch (e) {
  console.log("redefine: threw", oa.propertyIsEnumerable("p"));
}
try {
  delete oa.p;
  console.log("delete: no throw");
} catch (e) {
  console.log("delete: threw", oa.p);
}

// §7.3.14 moves configurable the same way for an accessor member.
const f = { get s() { return 8; }, t: 3 };
Object.seal(f as any);
const ds = Object.getOwnPropertyDescriptor(f as any, "s");
console.log(ds.enumerable, ds.configurable, (f as any).s);
