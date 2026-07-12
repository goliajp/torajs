// ES2025 set-method reflection + any-tier dispatch: the seven
// methods exist as Set.prototype own properties with spec
// name/length, are callable through an any receiver, and reject a
// non-Set argument with a TypeError.

const sp: any = Set.prototype;
console.log(typeof sp.union, typeof sp.intersection, typeof sp.difference);
console.log(typeof sp.symmetricDifference, typeof sp.isSubsetOf, typeof sp.isSupersetOf, typeof sp.isDisjointFrom);
console.log(sp.union.name, sp.union.length);
console.log(sp.symmetricDifference.name, sp.symmetricDifference.length);
console.log(sp.isDisjointFrom.name, sp.isDisjointFrom.length);

const d = Object.getOwnPropertyDescriptor(Set.prototype, "union") as any;
console.log(typeof d.value, d.writable, d.enumerable, d.configurable);

// any-tier calls hit the same kernels as the static path
const a: any = new Set([1, 2, 3]);
const b: any = new Set([2, 3, 4]);
const u = a.union(b);
console.log(u.size, u.has(1), u.has(4));
const i = a.intersection(b);
console.log(i.size, i.has(2), i.has(1));
const df = a.difference(b);
console.log(df.size, df.has(1), df.has(2));
const sd = a.symmetricDifference(b);
console.log(sd.size, sd.has(1), sd.has(4), sd.has(2));
console.log(a.isSubsetOf(b), a.isSupersetOf(b), a.isDisjointFrom(b));
const sub: any = new Set([2, 3]);
console.log(sub.isSubsetOf(a), a.isSupersetOf(sub), a.isDisjointFrom(new Set([9])));

// non-Set argument answers a catchable TypeError
try {
  a.union(42);
  console.log("no throw");
} catch (e: any) {
  console.log("TypeError caught");
}
